# Design: Two Streams Architecture

*The raw fact and the folded meaning.*

## The Insight

Every agent session produces two streams of data:

**Stream 1: What happened.** The raw CloudEvents — immutable, append-only, one per agent action. "Assistant sent tool_use Bash." "User sent tool_result ok." "System turn complete." These are facts. They never change.

**Stream 2: What it means.** StructuralTurns — derived by folding Stream 1 through the eval-apply detector. "Claude wrote 3 files after reading 6 sources, because the user asked to implement it in Scheme." These are interpretations. They're computed from facts by a deterministic pure function.

Stream 1 is the source of truth. Stream 2 is a projection. If the fold function changes (better sentence building, richer domain events, a new understanding of what a "turn" means), you replay Stream 1 and get a new Stream 2. The facts don't move. The meaning does.

Both streams are exposed. Both are persistent. Both are replayable. The user sees both — raw events for debugging, folded turns for understanding.

## Architecture

```
The world              The listener           Stream 1              The fold              Stream 2
(agents working)       (pure source)          (raw facts)           (pure function)       (meaning)

┌─────────────┐      ┌───────────────┐      ┌──────────────┐      ┌──────────────┐      ┌───────────────┐
│ Claude Code │─────▶│ Watcher/Hooks │─────▶│ cloud_events │─────▶│  step()      │─────▶│ structural    │
│ Pi-mono     │      │ reader.rs     │      │              │      │              │      │ _turns        │
│ Future agent│      │               │      │ Append-only  │      │ Pure fold:   │      │               │
└─────────────┘      │ Translates    │      │ Immutable    │      │ (acc, event) │      │ Recomputable  │
                     │ bytes →       │      │ Replayable   │      │   → acc'     │      │ Deterministic │
                     │ CloudEvents   │      │              │      │              │      │               │
                     │               │      │ One per      │      │ Stateful but │      │ One per       │
                     │ No logic      │      │ agent action │      │ pure — same  │      │ coalgebra     │
                     │ beyond format │      │              │      │ input, same  │      │ step          │
                     └───────────────┘      └──────────────┘      │ output.      │      └───────┬───────┘
                                                                  │ Always.      │              │
                                                                  └──────────────┘              │
                                                                                               │
                                                                                     ┌─────────▼─────────┐
                                                                                     │ sentence()        │
                                                                                     │                   │
                                                                                     │ Stateless map:    │
                                                                                     │ turn → sentence   │
                                                                                     │                   │
                                                                                     │ No accumulation.  │
                                                                                     │ Pure function.    │
                                                                                     └─────────┬─────────┘
                                                                                               │
                                                                                     ┌─────────▼─────────┐
                                                                                     │ sentence_patterns │
                                                                                     └───────────────────┘
```

## The Fold

The core of the system is one pure function:

```
step : (Accumulator, CloudEvent) → StepResult

where StepResult = Continue(Accumulator)
                 | TurnComplete { accumulator, turn, patterns }
```

This is the coalgebra from SICP. Each call either continues accumulating (the loop takes another step) or completes a turn (the coalgebra yields a value). The function is deterministic: same accumulator + same event = same result. Always.

The Accumulator holds the pending turn being assembled:
- Human message (if the user spoke)
- Thinking record (if the model reasoned)
- Eval output (what the model said/decided)
- Pending applies (tool calls awaiting results)
- Completed applies (tool calls with results)
- Environment tracking (message count, timestamps)

When a `system.turn.complete` event arrives, the accumulator crystallizes into a `StructuralTurn` — the completed coalgebra step. The accumulator resets. The turn is emitted to Stream 2.

### Why the fold must be pure

If the fold is pure, then:

1. **Replayability.** Delete Stream 2, replay Stream 1, get the same Stream 2 back. The projection is reproducible.

2. **Testability.** Feed a sequence of CloudEvents to `step()`, assert on the output. No mocks, no I/O, no setup. The probability-class test fixtures are exactly this.

3. **Auditability.** "Why did it say Claude *wrote* instead of *edited*?" Trace the StructuralTurn back through the fold to the specific CloudEvents that produced it. The chain is deterministic and inspectable.

4. **Evolvability.** Change the fold function, replay, get new meaning from old facts. The interpretation improves without the data changing.

## The Actor

The fold is math. The actor is lifecycle.

```rust
// The actor drives the fold
loop {
    let event = cloud_events_topic.next().await;
    let result = step(accumulator, &event);
    match result {
        StepResult::Continue(new_acc) => {
            accumulator = new_acc;
        }
        StepResult::TurnComplete { acc, turn, patterns } => {
            accumulator = acc;
            turns_topic.publish(&turn).await;
            patterns_topic.publish(&patterns).await;
            checkpoint(&accumulator).await;  // durability
        }
    }
}
```

The actor reads from the cloud_events topic, calls the pure fold, publishes results to output topics, and checkpoints its accumulator for crash recovery.

The actor doesn't contain logic. It's infrastructure. Swappable. You can drive the same `step()` function from:
- An in-process loop (today — `feed_cloud_event`)
- A NATS JetStream consumer (distributed deployment)
- A test harness (probability-class tests)
- A CLI replay tool (`open-story replay --session ca2bc88e`)

The fold function doesn't care who's calling it.

## The Sentence Layer

The sentence actor is simpler than the eval-apply actor because it's a **stateless map**, not a fold:

```
sentence : StructuralTurn → TurnSentence
```

Each turn maps to exactly one sentence. No accumulation. No state. No crash recovery needed — just replay the turns.

This is the algebra applied to the coalgebra's output. The coalgebra unfolds the event stream into turns. The algebra folds each turn into a sentence. Two directions of the arrow, composing.

## Persistence: JSONL as Topics

Each stream persists as JSONL files — one file per session, append-only:

```
data/
  cloud_events/           ← Stream 1: source of truth
    {session_id}.jsonl
  
  structural_turns/       ← Stream 2: recomputable projection
    {session_id}.jsonl
  
  patterns/               ← annotations on both streams
    {session_id}.jsonl
```

Properties:
- **Append-only.** Events are only added, never modified or deleted.
- **Replayable.** Read from line 0 and replay through the fold.
- **Human-readable.** `cat`, `grep`, `jq` work on these files.
- **Portable.** No database required. Copy the directory.
- **Recomputable.** Delete `structural_turns/`, replay from `cloud_events/`, regenerate.

SQLite mirrors this for indexed queries (the API reads from SQLite). But the JSONL files are the durable, portable representation. SQLite is a cache that can be rebuilt.

When NATS is available, the JSONL files become the consumer's local log — written by the NATS subscriber, replayed on startup. Same topology, different transport.

## The API

The API exposes both streams:

| Endpoint | Source | What it returns |
|----------|--------|-----------------|
| `GET /sessions/{id}/events` | Stream 1 (cloud_events) | Raw CloudEvents |
| `GET /sessions/{id}/turns` | Stream 2 (structural_turns) | StructuralTurns |
| `GET /sessions/{id}/patterns` | Both streams | PatternEvents (eval_apply, sentence, etc.) |
| `WS /ws` | Stream 1 (live) | Real-time CloudEvents |

The UI consumes both:
- **Live tab:** WebSocket stream of CloudEvents (Stream 1, real-time)
- **Story tab:** `/turns` + `/patterns?type=turn.sentence` (Stream 2, folded meaning)
- **Explore tab:** `/events` + `/patterns` (Stream 1, historical with annotations)

## The Five Layers, Revisited

```
Layer 5: Sentence     stateless map: turn → sentence        (algebra on the coalgebra's output)
Layer 4: Domain       ToolOutcome on each apply              (facts derived at translate time)
Layer 3: Structure    step(): (acc, event) → StepResult      (the coalgebra — the pure fold)
Layer 2: Events       CloudEvent stream                      (Stream 1 — the source of truth)
Layer 1: Raw          JSONL transcript bytes                  (agent's native format)
```

Layer 3 is the heart. It's where the raw event stream becomes structured meaning. Everything above it (sentences, domain events) is a projection of its output. Everything below it (CloudEvents, raw bytes) is its input.

The fold is the lens. Change the fold, change the meaning. The facts stay the same.

## What This Enables

An organization runs OpenStory across all its agent-augmented work. Every coding session, every research conversation, every automated workflow produces CloudEvents (Stream 1). The eval-apply fold processes them into StructuralTurns (Stream 2). The sentence layer produces human-readable summaries.

A manager opens the Story view and sees:
```
Turn 14: Claude wrote 3 Rust files after reading 6 sources, while testing 4 checks → continued
Turn 15: Claude committed changes, because "looks good, ship it" → answered
```

A developer opens the Live view and sees the raw events streaming in real time — tool calls, thinking blocks, model responses. Full visibility into what the agent is doing right now.

A researcher replays a session with a different fold function — one that tracks decision quality, or error recovery patterns, or collaboration dynamics — and gets new meaning from the same data.

The data is theirs. The meaning is computable. The process is observable. That's sovereignty.

## Implementation Path

### Phase 1: Pure fold extraction (current work)

Extract `step()` from `feed_cloud_event()`. The probability-class tests are the regression suite. The existing `EvalApplyDetector` becomes the actor that drives the fold.

Files: `rs/patterns/src/eval_apply.rs`
Tests: `rs/tests/test_eval_apply_probability.rs`

### Phase 2: Two-stream persistence

Separate JSONL output into `cloud_events/` and `structural_turns/` directories. The watcher writes to `cloud_events/`. The fold writes to `structural_turns/`. SQLite mirrors both for indexed queries.

Files: `rs/store/src/`, `rs/server/src/ingest.rs`

### Phase 3: Replay infrastructure

`open-story replay --session {id}` reads from `cloud_events/{id}.jsonl`, runs the fold, writes to `structural_turns/{id}.jsonl`. Validates that the fold is deterministic. Enables recomputation when the fold changes.

### Phase 4: NATS topic mapping

Map JSONL directories to NATS JetStream subjects. `cloud_events/{session_id}` becomes `open-story.events.{session_id}`. `structural_turns/{session_id}` becomes `open-story.turns.{session_id}`. The actor subscribes to the events subject and publishes to the turns subject. Same topology, distributed transport.

### Phase 5: Story UI

React components consuming `/turns` and `/patterns`. The prototype HTML (`scripts/story_html.py`) is the design spec. The data contract is already validated by the probability-class tests and the E2E script.

## Relationship to the Scheme Prototype

| Scheme concept | This architecture |
|---------------|-------------------|
| `agent-step` (04-eval-apply.scm) | `step()` — the pure fold function |
| `run-agent-loop` (the unfold) | The actor loop driving `step()` |
| The environment (list of messages) | The Accumulator (PendingTurn + counters) |
| `fold-stream` (02-stream.scm) | Stream 1 → Stream 2 (the fold over CloudEvents) |
| `compact-if-needed` (05-compact.scm) | Compaction events in the stream |
| `make-agent-tool` (06-agent-tool.scm) | Scope tracking in the Accumulator |
| `buildSentence` (sentence.ts) | The sentence actor (stateless map) |

The architecture is the Scheme prototype made durable and distributed. Same algebra, same coalgebra, same fixed point. Now with persistence, replay, and a REST API.
