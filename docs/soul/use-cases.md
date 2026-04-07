# Use Cases: Principles in Practice

These are concrete examples from the codebase demonstrating each principle. They are living references — when the code changes, these must be updated. If a use case points to a file or line that no longer exists, the use case is stale and must be refreshed.

Last verified: 2026-04-06

---

## 1. Observe, never interfere

**Where to look:** `rs/core/src/reader.rs`

The `read_new_lines()` function advances through transcript files using byte-offset seeking. It reads forward, translates what it finds into CloudEvents, and never writes back. When it encounters a partial line (no trailing newline), it refuses to consume it — the byte offset stays put for the next read (line 50-52):

```rust
// Partial line check: if the line doesn't end with \n, it's incomplete.
// Do NOT advance byte_offset — re-read next time.
if !line_buf.ends_with('\n') {
    break;
}
```

There is no code path from Open Story back to the agent's transcript files. The hooks endpoint (`rs/server/src/hooks.rs`) receives POSTed data but never sends commands. The entire data flow is unidirectional: Source → Translate → Ingest → Persist → Broadcast → Render. No arrow points back.

**What to verify:** Search for any `write`, `truncate`, or `modify` operations on transcript files. There should be zero. Search for any HTTP response from the hooks endpoint that contains commands or instructions. There should be none.

**Why it matters:** If the observer affects the observed, the observation is compromised. This is a functional constraint — the pipeline is a pure function of its input.

---

## 2. Behavior-Driven Development

**Where to look:** `ui/tests/bdd.ts`

The `scenario(given, when, then)` helper enforces pure data flow: Given returns context, When transforms it, Then asserts on the output. No shared mutable state, no hidden closures:

```typescript
export function scenario<G, W>(
  given: () => G,
  when: (context: G) => W,
  then: (result: W) => void,
): void {
  const context = given();
  const result = when(context);
  then(result);
}
```

**Boundary table example:** `ui/tests/lib/event-transforms.test.ts` (lines 33-43)

```typescript
describe("shortenCommand", () => {
  it.each([
    ["cd /home/user/project && cargo test", "cargo test"],
    ['cd "C:\\Users\\test" && npm run build', "npm run build"],
    ["plain command", "plain command"],
    ["", ""],
  ])("shortenCommand(%j) => %j", (input, expected) => {
    expect(shortenCommand(input)).toBe(expected);
  });
});
```

Every edge case in one place. The table IS the spec. The `scenario()` helper is used throughout — see `ui/tests/lib/event-transforms.test.ts` (lines 219-243) for `scenario()` applied to eventColor with git risk classification.

**What to verify:** New tests should use boundary tables for pure functions and `scenario()` for behavioral assertions. If a test file doesn't follow these patterns, ask why.

---

## 3. Artifact flow: Story → Plan → Implementation

**Where to look:**
- Backlog: `docs/BACKLOG.md` (Observability → Agent Behavior Patterns)
- Implementation: `rs/patterns/src/lib.rs` and the seven detector files in `rs/patterns/src/`
- Reports: `docs/research/sessions/` — moment-in-time analyses generated from running scripts against real sessions

The backlog describes what users need (pattern visibility) in a short paragraph. The patterns crate is the implementation. The reports in `docs/research/sessions/` show the patterns being consumed in practice — this is how the layers cohere from intent → implementation → use.

**What to verify:** `docs/BACKLOG.md` is the single source of truth for future work. When work is complete, the entry is removed — completed work lives in git history.

---

## 4. Actor systems and message-passing

**Where to look:** `rs/server/src/consumers/`

NATS JetStream is the spine. Sources publish CloudEvents to hierarchical subjects (`events.{project}.{session}.main`, `events.{project}.{session}.agent.{agent_id}`), and **four independent consumer actors** subscribe — each a tokio task with its own state and failure domain:

- `consumers/persist.rs` — subscribes to `events.>`, deduplicates, writes to SQLite + JSONL, indexes FTS5
- `consumers/patterns.rs` — runs the 7-detector pipeline, publishes `PatternEvent`s to `patterns.>`
- `consumers/projections.rs` — updates incremental `SessionProjection` (token counts, metadata, depths)
- `consumers/broadcast.rs` — assembles `WireRecord`s and pushes them to all WebSocket clients

Adding a new consumer means writing a new `consumers/foo.rs` that subscribes to a NATS subject. **No fan-out function to edit. No central registry. No `RwLock` to share.** State is held behind `Arc<dyn EventStore>` for lock-free concurrent SQLite access. Consumers don't reach into each other — the only contract is the NATS subject.

This is the architectural shift away from the old monolithic `ingest_events()` pipeline (which still exists in `rs/server/src/ingest.rs`, used only by the broadcast consumer for projection state — see BACKLOG: "Decompose Broadcast Consumer"). The decomposition is the actor model in its honest form: independent processes communicating through messages on a shared bus.

The UI side mirrors this: `ui/src/streams/connection.ts` uses RxJS subjects as message channels. `status$` and `messages$` are independent streams that consumers subscribe to.

**What to verify:** New consumers should be new files in `rs/server/src/consumers/`, subscribing to a NATS subject independently. No consumer should depend on another consumer's output. If you find yourself adding a `pub` field to one consumer so another can read it, you're building the wrong shape — publish a derived event to a new subject instead.

---

## 5. Functional-first, side effects at the edges

**Where to look:** `rs/views/src/from_cloud_event.rs`

The `from_cloud_event()` function (line 57) is a pure function: `&Value` in, `Vec<ViewRecord>` out. No I/O, no mutation, no AppState access. It normalizes legacy event types, extracts fields deterministically, and handles edge cases — all without touching anything outside its arguments.

The entire views crate has no side-effect dependencies. Check `rs/views/Cargo.toml` — only serde, chrono, and uuid.

**The dependency graph IS the enforcement:**
```
core (serde, chrono, uuid)        ← pure data types
views (core, serde)               ← pure transforms
patterns (views, serde)           ← pure fold over events
store (core, views, patterns)     ← persistence boundary
server (all above + axum, tokio)  ← all effects live here
```

An agent adding a feature to `views/` literally cannot import I/O libraries without modifying Cargo.toml. The compiler enforces the abstraction barrier.

**Lesson from pi-mono integration:** When adding pi-mono support, we initially mutated the `raw` field inside the translator — renaming `toolCall` → `tool_use`, restructuring content blocks — so the views layer wouldn't need changes. This broke purity: the translator's output no longer contained its input data. A pure translator *extracts and transforms*; it doesn't alter the source. The fix: translators leave `raw` untouched, set an `agent` discriminator on the CloudEvent, and the views layer branches on `agent` to parse each format. Format-awareness moved to where it belongs — the rendering boundary.

**What to verify:** Can you implement your feature in `views/` or `patterns/` without adding an I/O dependency? If yes, you're in the right place. If you need network, filesystem, or database access, the feature belongs in `server/` or `store/`. If you're tempted to mutate input data to make downstream code simpler, reconsider — add a discriminator and let downstream handle the branching.

---

## 6. Reactive and event-driven

**Where to look:** `ui/src/streams/connection.ts`

The WebSocket connection is an RxJS Observable chain (lines 41-72):

```typescript
const sub = timer(0)
  .pipe(
    tap(() => { status$.next("connecting"); }),
    switchMap(() => webSocket<WsMessage>({ url: wsUrl, ... })),
    tap((msg) => { messages$.next(msg); }),
    catchError((err) => { status$.next("disconnected"); return EMPTY; }),
    retry({ delay: () => timer(2000) }),
  )
  .subscribe();
```

Data flows one direction: source → observable → subjects → subscribers. The UI reacts to state changes — it never polls. Connection status, message handling, and error recovery are all expressed as stream transformations.

On the backend, `rs/server/src/broadcast.rs` manages a `tokio::sync::broadcast` channel. Events are pushed to all connected WebSocket clients. No polling, no request/response — pure event-driven push.

**What to verify:** New data flows should use observables and subjects (UI) or broadcast/mpsc channels (Rust). If you're tempted to add a polling loop or imperative state mutation, reconsider.

---

## 7. Open standards, user-owned data

**Where to look:** `rs/core/src/cloud_event.rs`

Every event is a CloudEvent 1.0 with required spec fields (lines 7-23):

```rust
pub struct CloudEvent {
    pub specversion: String,    // always "1.0"
    pub id: String,             // UUID
    pub source: String,
    pub event_type: String,     // "io.arc.event"
    pub time: String,           // RFC 3339
    pub datacontenttype: String, // "application/json"
    pub data: serde_json::Value,
    // optional extensions...
}
```

The boundary-table test (line 58) verifies spec compliance across field combinations.

Persistence formats are all open and portable:
- **JSONL** — append-only event log, grep-able (`rs/store/src/persistence.rs`)
- **SQLite** — durable queryable store (`rs/store/src/sqlite_store.rs`)
- **Markdown** — plans and documentation (`docs/`)
- **TOML** — configuration (`data/config.toml`)

**What to verify:** New event types must be valid CloudEvents. New persistence must use open, portable formats. If you're tempted to add a binary format or proprietary encoding, reconsider.

---

## 8. Minimal, honest code

**Where to look:** `ui/src/components/Timeline.tsx` and `rs/store/src/projection.rs`

Timeline.tsx renders event cards directly — each card IS the content. No expand/collapse state machine, no dual-window abstraction, no Show more/less toggles. The component does one thing simply.

SessionProjection in `rs/store/src/projection.rs` maintains pre-computed views (filter counts, depths, labels) that are updated incrementally on each `append()` call. It's a struct with an append method and query accessors. No trait hierarchy, no generic framework, no configuration system. The simplest thing that works.

Contrast with what was removed: the project previously had a lazy-loading list abstraction for sessions with 500-2000 records. The data fits in memory and renders in milliseconds. The abstraction solved a problem that didn't exist. It was deleted.

**What to verify:** Before adding a helper, abstraction, or layer, ask: "Does this solve a real problem, or a hypothetical one?" If you can't articulate the sovereignty benefit, it doesn't belong here. Three clear lines beat a clever helper.

---

## 9. Prototype first / Scripts over rawdogging

**Where to look:**
- Entry point: `scripts/sessionstory.py` (the first script to reach for when answering "what happened")
- Validator: `scripts/check_docs.py` (TDD docs validator — the second script worth knowing)
- Production: `rs/patterns/src/lib.rs` (the 7 streaming detectors that the analysis scripts query)

The `scripts/` directory is a working library of saved investigations. Two of them are the everyday entry points:

- **`sessionstory.py SESSION_ID`** hits the OpenStory REST API, aggregates records and patterns, and emits a structured fact sheet — record-type histogram, tool histogram, eval-apply patterns, turn phases, verbatim sentences from the `turn.sentence` detector, prompt timeline. It does not narrate; narration is the model's job. Add `--unfinished` to see trailing assistant messages, useful when picking up where a previous session left off.
- **`check_docs.py`** is a TDD docs validator. It compares claims in markdown (crate counts, detector counts, file references, NATS mentions, sessionstory pointers) against the live codebase (`rs/Cargo.toml` workspace members, `rs/patterns/src/*.rs` files, `rs/server/src/consumers/*.rs` files, the filesystem). When the docs drift from reality, it tells you which assertion failed.

Both follow the same shape: stdlib only, dataclasses for output, pure functions for the core logic, side effects at the edges, `--test` flag with synthetic-fixture self-tests. Both have project-level Claude Code skills at `.claude/skills/sessionstory/` and `.claude/skills/check-docs/` so any agent in the repo can invoke them.

The structure-analysis siblings — `analyze_eval_apply_shape.py`, `analyze_turn_shapes.py`, `analyze_event_groups.py`, `analyze_payload_sizes.py` — answer narrower questions and feed into bigger reports. `query_store.py` is the SQL escape hatch. `token_usage.py` is the cost ledger. See `README.md` for the grouped index.

**The lesson learned the hard way:** we built a tree abstraction for what turned out to be a linked list. We built a lazy-loading list for 2000 records that render in milliseconds. Both mistakes happened because we *imagined* the data instead of *looking* at it. Scripts in `scripts/` are how you look. Run one before you write a single line of production code.

**Sessions are also data the agent should query.** When you need to answer "what happened in the session that did X" — don't grep transcript files. Run `sessionstory.py SID` or hit `/api/sessions/{id}/records` directly. Dogfood the API. The product exists because raw transcripts aren't a useful representation of what happened; running the API on yourself is how you eat your own cooking.

**What to verify:** Before writing inline shell or Python for analysis, check `scripts/` for an existing script that answers your question. If you write a new analysis as a one-off, save it as a `scripts/foo.py` file with a `--test` flag and a docstring `Usage:` header. Raw shell vanishes; scripts endure.

---

## 10. Multi-agent observation (principles 1, 4, 5, 6, 7 in practice)

**Where to look:**
- Format detection: `rs/core/src/reader.rs` (lines 70-76)
- Pi-mono translator: `rs/core/src/translate_pi.rs`
- Agent-aware views: `rs/views/src/from_cloud_event.rs` (`extract_tool_calls`, `extract_tool_results`)
- Prototype: `scripts/translate_pi_mono.py`
- Config: `rs/server/src/config.rs` (`pi_watch_dir` field)
- Second watcher: `rs/src/server/mod.rs` (pi-mono watcher block)

Open Story observes multiple coding agents simultaneously. This feature exercises nearly every principle:

**Observe, never interfere (1):** The pi-mono watcher is read-only, just like the Claude Code watcher. It reads JSONL session files and never writes back. Two independent watchers, same unidirectional pipeline.

**Functional-first (4):** Each translator is a pure function — JSONL line in, CloudEvent out. `raw` is `line.clone()` always. We initially broke this by mutating `raw` to normalize pi-mono's field names into Claude Code's shape. The mutation was a hidden side effect that destroyed the original data. The fix: leave `raw` untouched, add an `agent` discriminator, and move format-awareness to the views layer.

**Reactive and event-driven (5):** Both watchers feed the same `ingest_events()` pipeline. Events from different agents flow through the same broadcast channel, same WebSocket, same UI. No polling, no special-casing at the transport level.

**Open standards, user-owned data (6):** Each agent's data stays in its native format inside `raw`. Pi-mono says `toolCall` and `arguments` — that's what's persisted. Claude Code says `tool_use` and `input` — that's what's persisted. The user's data is honest about its source.

**Minimal, honest code (7):** Format differences are handled by two simple branches in `extract_tool_calls` and `extract_tool_results`, keyed on the `agent` field. No abstraction layer, no format registry, no plugin system. A match statement.

**Abstraction barrier (SICP):** The translate layer proved its value as an abstraction barrier. Everything above it (ingest, store, patterns, projections, broadcast, UI) sees CloudEvents. Everything below it (file watchers, raw JSONL) deals with agent-specific formats. When pi-mono was added, a new translator was created and the views layer learned to branch on `agent` — nothing else in the pipeline changed. The barrier held. See `docs/soul/sicp-lessons.md` for the theoretical foundation.

**Lessons learned:**
- We initially mutated `raw` in the pi-mono translator to reshape content blocks into Claude Code's format. This broke functional purity — the translator's output no longer contained its input data. It also violated data sovereignty — `raw` should preserve exactly what the agent wrote. The fix: add the `agent` discriminator and let the views layer handle format differences.
- We also normalized pi-mono's field names (`input` → `input_tokens`, `toolCall` → `tool_use`) to avoid changing the views layer. This distorts agent-specific data and creates false assumptions about compatibility. Different agents have legitimately different structures. Preserve them.

**Configuration:**
- `watch_dir` — Claude Code transcripts (default `~/.claude/projects/`)
- `pi_watch_dir` — Pi-mono sessions (default empty, set via `data/config.toml` or `OPEN_STORY_PI_WATCH_DIR` env var)

Both watchers run concurrently, feeding the same ingest pipeline.

**What to verify:** When adding a new agent format, follow this pattern: write a prototype script in `scripts/`, create a translator in `rs/core/src/`, add format detection in `reader.rs`, and add agent-specific branches in `from_cloud_event.rs`. Never mutate `raw`. Never normalize agent-specific field names. The `agent` field is the discriminator.
