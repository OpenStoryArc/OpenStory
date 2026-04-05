# Design: Domain Events & Narrative View

*Adding "what changed in the world" to OpenStory's event pipeline, and a Story tab to make it visible.*

## Background

This design comes from a research session exploring the computational structure of AI coding agent sessions (see [the prototype and open letter](https://github.com/maxglassie/claurst/tree/scheme-metacircular-evaluator/scheme)). The key insight: agent sessions have layered structure that OpenStory can surface.

Currently, OpenStory has **technical events** — `tool_call`, `tool_result`, `user_message`, `assistant_message`. These describe what the machine did. They're the right foundation.

This design adds two things:
1. **Domain events** — deterministic facts about what changed in the world (`FileCreated`, `CommandExecuted`, etc.)
2. **A Story tab** — a narrative view of sessions built on turns, domain events, and natural language summaries

Both are additive. Nothing in the existing pipeline changes.

## Part 1: Domain Events (Rust)

### What They Are

A domain event is a deterministic projection derived from a `tool_call` + `tool_result` pair. The mapping is a pure function — same input, same output, no heuristics.

| Tool | Result contains | Domain Event |
|------|----------------|-------------|
| Write | "created successfully" | `FileCreated { path }` |
| Write | "updated successfully" | `FileModified { path }` |
| Write | is_error: true | `FileWriteFailed { path, reason }` |
| Edit | (any success) | `FileModified { path }` |
| Edit | is_error: true | `FileWriteFailed { path, reason }` |
| Read | (any success) | `FileRead { path }` |
| Read | is_error: true | `FileReadFailed { path, reason }` |
| Grep, Glob | (any) | `SearchPerformed { pattern, source: "filesystem" }` |
| WebSearch, WebFetch | (any) | `SearchPerformed { pattern, source: "web" }` |
| Bash | (any) | `CommandExecuted { command, succeeded }` |
| Agent | (any) | `SubAgentSpawned { description }` |

This mapping was prototyped and tested in TypeScript (20 tests, all passing) at `claurst/scheme/prototype/domain.ts`.

### Where It Lives in the Codebase

**New type** in `rs/core/src/event_data.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolOutcome {
    FileCreated { path: String },
    FileModified { path: String },
    FileRead { path: String },
    FileWriteFailed { path: String, reason: String },
    FileReadFailed { path: String, reason: String },
    SearchPerformed { pattern: String, source: String },
    CommandExecuted { command: String, succeeded: bool },
    SubAgentSpawned { description: String },
}
```

**New field** on `ClaudeCodePayload` and `PiMonoPayload`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub tool_outcome: Option<ToolOutcome>,
```

Same pattern as every other optional field. Same `skip_serializing_if`. Same `#[serde(default)]`.

**New function** in `rs/core/src/translate.rs`:

```rust
fn derive_tool_outcome(
    tool_name: &str,
    tool_input: &Value,
    result_output: &str,
    is_error: bool,
) -> Option<ToolOutcome> {
    match tool_name {
        "Write" => {
            let path = tool_input.get("file_path")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if is_error {
                Some(ToolOutcome::FileWriteFailed { path, reason: result_output.to_string() })
            } else if result_output.contains("created successfully") {
                Some(ToolOutcome::FileCreated { path })
            } else {
                Some(ToolOutcome::FileModified { path })
            }
        }
        "Edit" => {
            let path = tool_input.get("file_path")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if is_error {
                Some(ToolOutcome::FileWriteFailed { path, reason: result_output.to_string() })
            } else {
                Some(ToolOutcome::FileModified { path })
            }
        }
        "Read" => {
            let path = tool_input.get("file_path")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            if is_error {
                Some(ToolOutcome::FileReadFailed { path, reason: result_output.to_string() })
            } else {
                Some(ToolOutcome::FileRead { path })
            }
        }
        "Grep" | "Glob" => {
            let pattern = tool_input.get("pattern")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some(ToolOutcome::SearchPerformed { pattern, source: "filesystem".to_string() })
        }
        "WebSearch" | "WebFetch" => {
            let query = tool_input.get("query")
                .or_else(|| tool_input.get("url"))
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some(ToolOutcome::SearchPerformed { pattern: query, source: "web".to_string() })
        }
        "Bash" => {
            let command = tool_input.get("command")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some(ToolOutcome::CommandExecuted { command, succeeded: !is_error })
        }
        "Agent" => {
            let description = tool_input.get("description")
                .and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some(ToolOutcome::SubAgentSpawned { description })
        }
        _ => None,
    }
}
```

Follows the existing `apply_*` pattern from the monadic EventData refactor. Called in the tool_result branch of `translate_line()`.

**Same pattern for pi-mono** in `translate_pi.rs`, adapted for pi-mono's tool names (`exec`, `read`, `write`, `edit`).

**Surface in views**: Add `tool_outcome: Option<ToolOutcome>` to the `ToolResult` struct in `rs/views/src/view_record.rs`. Extract from payload in `from_cloud_event.rs`.

### Implementation Steps (Rust)

1. Add `ToolOutcome` enum to `event_data.rs`
2. Add `tool_outcome` field to both payload structs, update `new()` constructors
3. Add `derive_tool_outcome()` to `translate.rs`, call in tool_result branch
4. Add `derive_tool_outcome_pi()` to `translate_pi.rs`
5. Surface on `ToolResult` in views layer
6. Tests: unit tests for every tool type mapping + integration tests with fixtures
7. `cargo test` — all existing + new tests pass

### What This Enables

- **Pattern detectors** can match on `ToolOutcome` directly instead of parsing tool names and result strings
- **Session projection** can track files created/modified/read as aggregate counts
- **API queries** like "which sessions created files in `src/`" become possible
- **The Story tab** (Part 2) builds on domain events for its narrative layer

---

## Part 2: Story Tab (TypeScript/React)

### What It Is

A new tab alongside Live and Explore that shows sessions as **narrative structure** rather than event streams. Same data, different lens.

```
Live:    "Here's what's happening right now"    (real-time event stream)
Explore: "Here's what happened in this session" (historical browse + search)
Story:   "Here's what it meant"                 (narrative with five layers)
```

### The Five Layers

Each turn in the Story view shows five layers of the same data:

1. **Sentence** — natural language one-liner: "Claude wrote 8 Scheme files after reading 6 sources → answered"
2. **Diagram** — expandable grammatical tree (subject → verb → object, subordinate clauses)
3. **Domain facts** — badge strip: `+3 created ~1 modified 2 cmd ok` (from Part 1)
4. **Domain events** — expandable list of deterministic facts: `+ created 01-types.scm`
5. **Phases** — eval-apply structure: HUMAN (blue), THINKING (purple), EVAL (green), APPLY (amber)

All five layers are independently toggleable. Each answers a different question for a different audience.

### Turn Grouping

Events are grouped by `turn_end` records — the real coalgebra step boundary. Between two `turn_end`s, there may be multiple eval-apply cycles (model responds → calls tools → sees results → responds again). All of that is ONE turn.

This is different from the Live/Explore view where every event is a separate card.

### Data Pipeline

```
ViewRecords (from WebSocket or REST)
    │
    ▼
Eval-Apply Detector (pure fold)
    │
    ▼
StructuralTurns[]
    │
    ├──▶ Domain Events (deterministic — from ToolOutcome on payload)
    │
    └──▶ Sentence Builder (interpretive — tool classification + grammar)
    │
    ▼
StoryView React component
```

### The Eval-Apply Detector

A pure fold: `(state, record) → (state, structuralEvents[])`. Same pattern as `fold-left` in functional programming — accumulate state over a sequence of records.

The detector classifies each record into a computational phase:
- `user_message` → **human** (the input to eval)
- `reasoning` → **thinking** (model reasoning before responding)
- `assistant_message` → **eval** (model examined environment, produced expression)
- `tool_call` → **apply** (tool dispatched)
- `tool_result` → apply complete (env grows)
- `turn_end` → **turn boundary** (coalgebra step complete)

Prototyped and tested: `claurst/scheme/prototype/eval-apply-detector.ts` (18 tests).

### The Sentence Builder

Classifies tool calls by **narrative role** and composes a grammatical sentence:

| Role | Tools | Sentence position |
|------|-------|-------------------|
| preparatory | Read, Grep, Glob, WebSearch | "after reading..." (subordinate clause) |
| creative | Write, Edit, git commit | "wrote..." (main verb) |
| verificatory | Bash(test), Bash(build) | "while testing..." (subordinate clause) |
| delegatory | Agent | "by delegating to..." (subordinate clause) |

The dominant role determines the verb. Non-dominant roles become subordinate clauses. The human message is the adverbial ("because '...'"). The stop reason is the predicate ("→ answered" / "→ continued").

Prototyped and tested: `claurst/scheme/prototype/sentence.ts` (20 tests).

**Important:** The sentence layer is heuristic (interpretation), not deterministic. The domain events layer below it IS deterministic. The separation is intentional — facts are facts, sentences are stories about facts.

### New Files

```
ui/src/lib/story/
  types.ts          — StructuralTurn, DomainTurn, TurnSentence
  detector.ts       — eval-apply fold (port from prototype)
  domain.ts         — domain event derivation (port from prototype)  
  sentence.ts       — sentence builder (port from prototype)
  transform.ts      — ViewRecord[] → StorySessionState

ui/src/components/story/
  StoryView.tsx      — main view: header + turn list
  TurnCard.tsx       — one turn: sentence + phases + domain
  SentenceLine.tsx   — italic one-liner
  SentenceDiagram.tsx — expandable Reed-Kellogg tree
  DomainStrip.tsx    — badge strip
  DomainEvents.tsx   — expandable fact list
  PhaseBlock.tsx     — HUMAN / THINKING / EVAL / APPLY
  EnvBar.tsx         — environment size bar
```

### Implementation Steps (TypeScript/React)

1. Port `types.ts`, `detector.ts`, `domain.ts`, `sentence.ts` from prototype into `ui/src/lib/story/`
2. Write `transform.ts` adapter: ViewRecord → ApiRecord shape
3. Port prototype tests to Vitest in `ui/tests/lib/story/`
4. Build `TurnCard.tsx` + `PhaseBlock.tsx` — basic turn rendering
5. Build `SentenceLine.tsx` + `SentenceDiagram.tsx` — narrative layer
6. Build `DomainStrip.tsx` + `DomainEvents.tsx` — facts layer
7. Build `StoryView.tsx` — compose everything with session header
8. Add routing: `/story/:id` tab alongside Live and Explore
9. Wire data: fetch records via REST, run through transform pipeline
10. Polish: collapse/expand, layer toggles, responsive, keyboard nav

### Dependency on Part 1

The Story tab can work WITHOUT the Rust domain events — the prototype derives domain events client-side from tool call names and result strings. But WITH `ToolOutcome` on the payload (Part 1), the derivation is pre-computed and authoritative. The UI just reads the field instead of re-deriving.

Recommended: implement Part 1 first (small, contained, backend-only), then Part 2 consumes it.

---

## Relationship to Existing Features

| Existing | Domain Events add | Story Tab adds |
|----------|-------------------|----------------|
| Tool call cards in timeline | `ToolOutcome` on each result | Tools grouped by turn with narrative roles |
| Session Activity Summary | File counts from domain events | Full turn-by-turn narrative |
| Pattern detection (test cycles, etc.) | Cleaner matching on `ToolOutcome` | Patterns integrated into turn context |
| Workspace Impact Summary (backlog) | This IS the data layer for it | This IS the UI for it |
| Session Replay (backlog) | Domain events as replay units | Turn-based replay with narrative |

## Verification

### Part 1
- `cargo test` — all existing + new tests pass
- Query a real session's events — `tool_outcome` present on tool_result records
- Domain events match what the TypeScript prototype produces for the same session

### Part 2
- `npm test` — all Vitest tests pass (ported from prototype)
- Story tab renders our test session (`ca2bc88e`) with ~42 turns
- Sentences match prototype HTML output
- Each layer independently toggleable
- Compare with Live/Explore — same session, same events, different view
