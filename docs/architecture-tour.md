# Open Story Architecture Tour

A guided walkthrough of the open-story codebase. Open this with Claude Code and follow along — read each file, ask questions, and build your mental model step by step.

**How to use this tour:** Open Claude Code in the project directory and say:
> "I'm reading docs/architecture-tour.md — let's start at Stop 1."

Claude will read the referenced files and explain them. Ask questions at any stop before moving on.

---

## The Big Picture

Open Story is a **real-time observer** for AI coding agents. It watches what coding agents do — every tool call, every file edit, every decision — and surfaces it in a live dashboard. It never interferes with the agent. It just watches.

```
                    ┌────────────────── NATS JetStream ──────────────────┐
                    │   events.{project}.{session}.>                     │
                    │   patterns.{project}.{session}.>                   │
                    │   changes.{project}.{session}.>                    │
                    └─────────────────────────┬──────────────────────────┘
                                              │
   ┌──────────┐    ┌─────────┐      publish   │   subscribe (independent consumers)
   │ Watcher  │───▶│         │───CloudEvent──▶│
   │ (notify) │    │ translate│                │   ┌─────────────┐
   └──────────┘    │   .rs   │                │──▶│  persist    │──▶ SQLite + JSONL + FTS5
                   │         │                │   └─────────────┘
   ┌──────────┐    │         │                │
   │  /hooks  │───▶│         │                │   ┌─────────────┐
   │ (HTTP)   │    └─────────┘                │──▶│  patterns   │──▶ 7 detectors → PatternEvent
   └──────────┘                               │   └─────────────┘
                                              │
                                              │   ┌─────────────┐
                                              │──▶│ projections │──▶ SessionProjection state
                                              │   └─────────────┘
                                              │
                                              │   ┌─────────────┐
                                              │──▶│  broadcast  │──▶ WireRecord → WS → React UI
                                              │   └─────────────┘
```

Two **sources** feed CloudEvents into NATS:
1. **File watchers** — monitor configured directories for JSONL transcript changes (one per agent)
2. **HTTP hooks** — coding agents POST events directly via `/hooks` (currently Claude Code only)

Both convert raw transcript lines into CloudEvents (CloudEvents 1.0 spec, with an `agent` extension attribute) and **publish to NATS JetStream** on a hierarchical subject (`events.{project_id}.{session_id}.main` for the main agent, `events.{project_id}.{session_id}.agent.{agent_id}` for subagents). A subscription to `events.{project}.{session}.>` reads main + all subagents.

Four **independent consumer actors** subscribe to NATS:
- `persist` — SQLite + JSONL + FTS5 indexing (deduplication, durable storage)
- `patterns` — runs the 7 streaming detectors and writes `PatternEvent`s
- `projections` — incremental `SessionProjection` (token counts, metadata, depths, filter counts)
- `broadcast` — assembles `WireRecord`s and pushes them to all WebSocket clients

Each consumer is a tokio task with its own failure domain. **No shared `RwLock` between actors.** State is held behind `Arc<dyn EventStore>` for lock-free concurrent SQLite access. NATS is the only contract between sources and consumers — adding a new consumer means adding a new subscriber, not editing a fan-out function.

This is the core architectural shift in the project: from a monolithic `ingest_events()` that owned dedup + persist + project + broadcast as one function, to **NATS as the spine** with independent actors subscribing to the same subjects.

---

## Stop 1: Entry Point

**File:** `rs/cli/src/main.rs`

This is where the binary starts. It's intentionally thin — just CLI arg parsing and delegation. Two modes:
- `open-story serve` (default) — runs the HTTP/WS server with dashboard
- `open-story watch` — watches files and emits CloudEvents to stdout

Read this file first. Notice how little it does — all the logic lives in the library crate.

**Questions to explore:**
- What's the default port?
- Where does it look for transcript files?
- What's the relationship between the `serve` and `watch` commands?

---

## Stop 2: The Watcher

**File:** `rs/src/watcher.rs` (delegates to `rs/core/` for reader + translator)

The file watcher uses the `notify` crate to monitor a directory tree for JSONL file changes. When a file is modified, it reads only the **new lines** (incremental reading via byte offset).

Key function: `watch_with_callback()` — this blocks forever on an OS-level file watch. That's why the server runs it on a `spawn_blocking` thread.

**Questions to explore:**
- How does it know which lines are new? (Look at `TranscriptState`)
- What's the `BACKFILL_WINDOW` and why does it exist?
- How does it derive session IDs and project IDs from file paths?

---

## Stop 3: The Reader

**File:** `rs/core/src/reader.rs` (re-exported via `rs/src/reader.rs`)

Incremental JSONL reader. Reads from the byte offset where it left off, parses each complete JSON line, and feeds them to the translator.

**Questions to explore:**
- What happens if a line is partially written (file still being appended to)?
- How does `TranscriptState` track position between reads?

---

## Stop 4: The Translator(s)

**File:** `rs/core/src/translate.rs` — Claude Code transcript → CloudEvents
**File:** `rs/core/src/translate_pi.rs` — Pi-mono session → CloudEvents

Each agent has its own translator. The reader (`reader.rs`) auto-detects the format on the first JSONL line and dispatches to the correct one. Both produce CloudEvents with the same subtype hierarchy, but preserve each agent's native field names in the extracted metadata and in `raw`.

Every CloudEvent carries an `agent` field (`"claude-code"` or `"pi-mono"`) so downstream code knows the source format. Raw data is never mutated — it's an untouched archive of the original transcript line.

**File:** `rs/core/src/cloud_event.rs`

The CloudEvent struct — the universal event format used internally. Includes the `agent` extension attribute.

**Questions to explore:**
- What subtypes exist? (e.g., `message.user.prompt`, `message.assistant.tool_use`)
- How does format detection work in `reader.rs`?
- How do the two translators differ in field extraction?
- Why CloudEvents and not a custom format?

---

## Stop 5: The Views Crate (BFF Transform)

This is a separate crate (`open-story-views`) that transforms untyped CloudEvents into **typed ViewRecords**. This is the Backend-For-Frontend boundary — the UI never parses raw transcript formats.

### 5a: RecordBody — the type system

**File:** `rs/views/src/unified.rs`

The discriminated union of all record types. This is the core type system:

```rust
enum RecordBody {
    ToolCall(ToolCall),           // name, input, raw_input, typed_input
    ToolResult(ToolResult),       // output, is_error
    UserMessage(UserMessage),     // content
    AssistantMessage(AssistantMessage),  // content blocks
    Reasoning(Reasoning),         // thinking content
    TurnEnd(TurnEnd),            // duration_ms
    TokenUsage(TokenUsage),      // input/output token counts
    // ... and more
}
```

Serialized with `#[serde(tag = "record_type", content = "payload")]` — so JSON looks like `{"record_type": "tool_call", "payload": {...}}`.

### 5b: The transform

**File:** `rs/views/src/from_cloud_event.rs`

`from_cloud_event(event) → Vec<ViewRecord>` — the main transform. Discriminates on CloudEvent subtype and produces typed records. One CloudEvent can produce multiple ViewRecords (e.g., an assistant message with 3 tool_use blocks → 3 ToolCall records).

### 5c: ViewRecord and WireRecord

**File:** `rs/views/src/view_record.rs`

The ViewRecord struct — typed event with session_id, agent_id, timestamp, and a RecordBody.

**File:** `rs/views/src/wire_record.rs`

WireRecord wraps ViewRecord with tree metadata:
- `depth` — how deep in the agent/subagent tree (0 = root)
- `parent_uuid` — parent event for tree reconstruction
- `truncated` — was the payload cut for wire transfer?
- `payload_bytes` — original size

Uses `#[serde(flatten)]` so ViewRecord fields appear at the top level in JSON.

### 5d: Typed tool inputs

**File:** `rs/views/src/tool_input.rs`

Parses raw tool inputs into typed structs (BashInput, EditInput, ReadInput, etc.). This powers the "one-line summary" in the timeline — e.g., showing the file path for a Read call instead of the raw JSON.

**Questions to explore:**
- How does `from_cloud_event` handle the two different source formats (transcript lines vs hook events)?
- What's the truncation threshold and why?
- How does `typed_input` differ from `raw_input`?

---

## Stop 6: The Server (and its consumers)

**File:** `rs/server/src/lib.rs` (entry point)

The server crate has two distinct halves:

### 6a: HTTP/WS surface

- `state.rs` — `AppState` boots from SQLite on restart, holds `Arc<dyn EventStore>` for lock-free concurrent access (no `RwLock` around the event store)
- `router.rs` — `build_router()` wires every HTTP and WebSocket route
- `api.rs` — REST endpoint handlers (`/api/sessions`, `/api/search`, `/api/agent/*`, …)
- `ws.rs` — WebSocket broadcast handler
- `hooks.rs` — `POST /hooks` receiver for coding agent HTTP hooks
- `config.rs` — `Config` struct + TOML loading
- `auth.rs` — Bearer token authentication middleware
- `metrics.rs` — Prometheus `/metrics` endpoint
- `broadcast.rs` — WebSocket broadcast channel management
- `ingest.rs` — legacy monolithic ingest path; only the broadcast consumer still uses it (see BACKLOG: "Decompose Broadcast Consumer")

### 6b: Independent consumer actors

**Directory:** `rs/server/src/consumers/`

Four tokio tasks, each subscribing to NATS independently. This is the post-NATS architecture — the replacement for the old `ingest_events()` fan-out:

| File | Subject filter | What it does |
|---|---|---|
| `persist.rs` | `events.>` | Dedup, write to SQLite + JSONL, index FTS5 |
| `patterns.rs` | `events.>` | Run the 7 detector pipeline, publish `PatternEvent`s to `patterns.>` |
| `projections.rs` | `events.>` | Update incremental `SessionProjection` (tokens, metadata, depths) |
| `broadcast.rs` | `events.>` | Assemble `WireRecord`, push to all WebSocket clients (still uses `ingest_events()` for projection state — see BACKLOG) |

Each consumer owns its own state. None reach into another's. The contract between them is the NATS subject — that's the only coupling.

**Questions to explore:**
- How does `projections` rebuild `SessionProjection` state from a NATS replay? (Hint: NATS JetStream's durable consumer + start sequence)
- Why does `broadcast` still use `ingest_events()`? (See BACKLOG: "Decompose Broadcast Consumer")
- How does `persist` deduplicate when both the file watcher and `/hooks` race to publish the same event?
- What happens when one consumer falls behind? (Independent failure domains — the others keep working)

---

## Stop 7: SessionProjection

**File:** `rs/store/src/projection.rs`

Incremental materialized view per session. Never recomputes from scratch — updated on every event append. Tracks:

- **Tree metadata** — depth and parent_uuid for every event (for the hierarchy view)
- **Filter counts** — 21 named filters (bash.git, tools, errors, agents, tests, etc.) with per-event deltas
- **Session label** — first user prompt (50 chars) + git branch
- **Token counts** — accumulated input/output tokens

The server-side filter counts drive badge numbers in the sidebar; client-side predicates in `ui/src/lib/timeline-filters.ts` drive instant filter switching without a network round trip.

**Questions to explore:**
- How does tree depth get computed from `parent_uuid` chains?
- What makes an event "ephemeral"?
- How do filter deltas flow to the UI?

---

## Stop 8: Pattern Detection

**File:** `rs/patterns/src/lib.rs` — Pipeline + types

Seven streaming detectors, each a pure state machine: `(state, event) → (new_state, patterns)`.

| Detector | File | What it finds |
|----------|------|---------------|
| EvalApply | `rs/patterns/src/eval_apply.rs` | SICP-style scope open/close, eval, apply, and turn-end events — the compound-procedure structure of the agent loop |
| Sentence | `rs/patterns/src/sentence.rs` | Per-turn natural-language summary like *"Claude edited translate.rs, after reading 3 sources, while testing 6 checks, because '…' → answered"* |
| TestCycle | `rs/patterns/src/test_cycle.rs` | edit → test → fail → fix loops |
| GitFlow | `rs/patterns/src/git_flow.rs` | git command sequences (status → add → commit → push) |
| ErrorRecovery | `rs/patterns/src/error_recovery.rs` | error → retry → success patterns |
| AgentDelegation | `rs/patterns/src/agent_delegation.rs` | subagent spawning |
| TurnPhase | `rs/patterns/src/turn_phase.rs` | classification of each turn into conversation / implementation / execution / testing / exploration / delegation |

Each detector implements the `Detector` trait with `feed()` and `flush()`. The patterns consumer (`rs/server/src/consumers/patterns.rs`) subscribes to NATS events, runs the pipeline, and persists `PatternEvent`s to the patterns stream + SQLite. The detectors themselves are pure — no NATS knowledge, no I/O.

The eval-apply and sentence detectors are the narrative layer — they're what the Story tab and `scripts/sessionstory.py` consume to summarize a session.

**Questions to explore:**
- How does the eval-apply detector decide when a scope opens vs closes?
- How does GitFlowDetector decide when a git workflow ends?
- What metadata does each pattern carry?
- How do patterns get associated back to individual timeline events?

---

## Stop 9: WebSocket Broadcasting

**File:** `rs/server/src/ws.rs`

Each connected dashboard client gets:
1. **Initial state** — full snapshot of records, filter counts, patterns, labels
2. **Live updates** — every new event as it's ingested

The `handle_socket()` function uses `tokio::select!` to multiplex between receiving broadcast messages and handling client disconnects.

**File:** `rs/server/src/hooks.rs`

The `/hooks` endpoint receives HTTP hooks from coding agents. These provide near-real-time events (vs. the file watcher which polls on file changes). Currently Claude Code is the only agent with hook support.

**Questions to explore:**
- What's in the initial state message?
- How does the broadcast channel work? What happens if a client is slow?
- How do hooks differ from file watcher events?

---

## Stop 10: REST API

**File:** `rs/server/src/api.rs`

REST endpoints read from in-memory state and SQLite (no heavy computation on requests):

| Group | Endpoint | Purpose |
|-------|----------|---------|
| **Sessions** | `GET /api/sessions` | List sessions with metadata, grouped by project |
| | `GET /api/sessions/{id}/events` | Raw CloudEvents |
| | `GET /api/sessions/{id}/summary` | Session summary (status, timing, counts) |
| | `GET /api/sessions/{id}/activity` | Tool breakdown + timeline stats |
| | `GET /api/sessions/{id}/tools` | Tool call distribution |
| | `GET /api/sessions/{id}/patterns` | Detected patterns (filterable by type) |
| | `GET /api/sessions/{id}/records` | WireRecords from projections |
| | `GET /api/sessions/{id}/view-records` | Typed ViewRecords |
| | `GET /api/sessions/{id}/conversation` | Conversation thread reconstruction |
| | `GET /api/sessions/{id}/transcript` | Raw transcript content |
| | `GET /api/sessions/{id}/file-changes` | Files modified during session |
| | `GET /api/sessions/{id}/meta` | Session metadata |
| | `GET /api/sessions/{id}/events/{eid}/content` | Full content for truncated records |
| | `GET /api/sessions/{id}/plans` | Plans extracted from this session |
| | `GET /api/sessions/{id}/synopsis` | Goal, journey, outcome summary |
| | `GET /api/sessions/{id}/tool-journey` | Tool usage over time |
| | `GET /api/sessions/{id}/file-impact` | File impact analysis |
| | `GET /api/sessions/{id}/errors` | Error events in session |
| | `GET /api/sessions/{id}/export` | Export session data |
| | `DELETE /api/sessions/{id}` | Delete a session |
| **Plans** | `GET /api/plans` | List all extracted plans |
| | `GET /api/plans/{id}` | Get a specific plan |
| **Insights** | `GET /api/insights/pulse` | Project activity pulse |
| | `GET /api/insights/tool-evolution` | Tool usage evolution over time |
| | `GET /api/insights/efficiency` | Session efficiency metrics |
| | `GET /api/insights/productivity` | Productivity metrics |
| **Agent** | `GET /api/agent/tools` | Tool schemas for agent consumption |
| | `GET /api/agent/project-context` | Project context for agents |
| | `GET /api/agent/recent-files` | Recently modified files |
| | `GET /api/agent/search` | Agent-optimized search |
| **Search** | `GET /api/search` | Full-text + semantic event search |
| **Other** | `GET /api/tool-schemas` | Tool schema definitions |
| | `POST /hooks` | Coding agent HTTP hook receiver |
| | `GET /ws` | WebSocket live event stream |

**Questions to explore:**
- How does project grouping work?
- What does the activity summary contain?
- How does the truncated content lazy-load endpoint work?
- How do the `/api/agent/*` endpoints differ from their dashboard counterparts?

---

## Stop 11: Persistence

### 11a: SQLite EventStore (primary)

**File:** `rs/store/src/sqlite_store.rs`

The primary durable store. Holds events, sessions, and patterns in SQLite tables. Used for boot recovery, queries, and the CLI commands (`synopsis`, `pulse`, `context`).

### 11b: JSONL SessionStore (backup)

**File:** `rs/store/src/persistence.rs`

One JSONL file per session (`./data/{session_id}.jsonl`). Append-only backup that survives even if the SQLite DB is lost. Human-readable and grep-able.

### 11c: PlanStore

**File:** `rs/store/src/plan_store.rs`

Extracts and stores plans from ExitPlanMode tool calls. Markdown files in `./data/plans/`.

**Questions to explore:**
- Why dual-write to both SQLite and JSONL?
- How does `replay_boot_sessions()` repopulate in-memory state from SQLite on startup?
- How are plans detected in the event stream?

---

## Stop 12: The React Dashboard

Now we cross into TypeScript. The UI is a single-page React app (Vite, TailwindCSS v4, RxJS).

### 12a: WebSocket connection

**File:** `ui/src/streams/connection.ts`

RxJS-based WebSocket client. Auto-reconnects on disconnect (2s delay). Exposes:
- `wsMessages$()` — observable of all incoming messages
- `connectionStatus$()` — "connecting" | "connected" | "disconnected"

### 12b: Message types

**File:** `ui/src/types/websocket.ts`

TypeScript mirrors of the Rust broadcast messages. `WsMessage` is the discriminated union.

**File:** `ui/src/types/wire-record.ts`

WireRecord and PatternView — the primary data types on the client.

**File:** `ui/src/types/view-record.ts`

RecordType, RecordPayload, ToolCall, ToolResult, etc. — the payload type system.

### 12c: State management

**File:** `ui/src/streams/sessions.ts`

`enrichedReducer()` — pure reducer that builds `EnrichedSessionState` from WebSocket messages. Initial state loads the snapshot; enriched messages incrementally append records, apply filter deltas, accumulate patterns.

No Redux, no Zustand — just a reducer + RxJS `scan()`.

### 12d: Timeline transform

**File:** `ui/src/lib/timeline.ts`

`toTimelineRows(records) → TimelineRow[]` — pure transform from WireRecords to renderable rows. Maps record_type to category (prompt/response/tool/result/thinking/system/error/turn), extracts summary text.

### 12e: Client-side filters

**File:** `ui/src/lib/timeline-filters.ts`

21 filter predicates that mirror the server-side filters. Instant switching — no network round trip. The server sends badge counts via filter_deltas; the client applies predicates locally.

### 12f: Components

**File:** `ui/src/components/Timeline.tsx` — Main timeline view (virtualized scrolling)
**File:** `ui/src/components/Sidebar.tsx` — Session list with sparklines + token badges
**File:** `ui/src/components/analytics/ActivitySummary.tsx` — Session stats + tool chart
**File:** `ui/src/components/events/GitFlowCard.tsx` — Git workflow visualization
**File:** `ui/src/components/DepthSparkline.tsx` — SVG depth profile sparkline

**Questions to explore:**
- How does the reducer handle ephemeral (progress) events differently from durable records?
- How does virtual scrolling work for large timelines?
- How do pattern badges get associated with timeline rows?

---

## Stop 13: Pure Logic Libraries

These are the testable units — pure functions with boundary-table tests.

| File | Purpose | Tests |
|------|---------|-------|
| `ui/src/lib/depth-profile.ts` | Max-in-bucket downsampling for sparklines | `ui/tests/lib/depth-profile.test.ts` |
| `ui/src/lib/turn-summary.ts` | Per-turn stats (tool calls, errors, edits) | `ui/tests/lib/turn-summary.test.ts` |
| `ui/src/lib/tool-chart-data.ts` | Sort + bucket tool distribution for charts | `ui/tests/lib/tool-chart-data.test.ts` |
| `ui/src/lib/git-flow-data.ts` | Parse git workflow metadata into steps | `ui/tests/lib/git-flow-data.test.ts` |
| `ui/src/lib/tool-colors.ts` | Tool name → Tokyonight color mapping | — |
| `ui/src/lib/subtree.ts` | Tree index + subtree focus (path compression) | `ui/tests/lib/subtree.test.ts` |
| `ui/src/lib/pattern-index.ts` | Event ID → pattern lookup index | `ui/tests/lib/pattern-index.test.ts` |

**Questions to explore:**
- What's the BDD `scenario(given, when, then)` pattern used in tests? (See `ui/tests/bdd.ts`)
- How do boundary tables work as specs?

---

## Stop 14: Test Infrastructure

### Rust tests
- Unit tests: inline `#[cfg(test)]` modules in each file
- Integration tests: `rs/tests/` with shared helpers in `rs/tests/helpers/mod.rs`
- `test_state()` creates isolated AppState with temp directory
- Container tests: `rs/tests/test_container.rs` (Docker-based, optional)

### UI tests
- Pure function tests: `ui/tests/lib/`, `ui/tests/streams/`
- BDD helper: `ui/tests/bdd.ts` — `scenario(given, when, then)`
- Run: `cd ui && npx vitest run` (or `npx vitest --ui` for visual mode)

### E2E tests
- Playwright: `e2e/tests/`
- Uses Docker container for backend + Vite for frontend
- Config: `e2e/playwright.config.ts`

**Run everything:**
```bash
cd rs && cargo test                    # Rust (~100 tests across 8 crates)
cd ui && npx vitest run                # UI (~500 tests)
cd e2e && npx playwright test          # E2E (~70 tests)
```

---

## Stop 15: Using OpenStory as an agent

If you're an AI agent working in this repo, OpenStory is *for you* as much as it is for the human. You should be using it to answer questions about past sessions, not grepping transcript files. The principle is **dogfood the API** — every observation about session behavior should come from the live REST API or from a script that reads it.

### The first thing to reach for: `scripts/sessionstory.py`

```bash
python3 scripts/sessionstory.py SESSION_ID            # markdown fact sheet
python3 scripts/sessionstory.py latest                # most recent session
python3 scripts/sessionstory.py SESSION_ID --json     # machine-readable
python3 scripts/sessionstory.py SESSION_ID --brief    # shape + prompts only
python3 scripts/sessionstory.py SESSION_ID --unfinished  # + trailing assistant messages
python3 scripts/sessionstory.py --list                # list recent sessions
```

`sessionstory.py` is the entry point for "what happened in this session." It hits three endpoints (`/api/sessions`, `/api/sessions/{id}/records`, `/api/sessions/{id}/patterns`), aggregates them deterministically, and emits a structured fact sheet — record-type histogram, tool histogram, eval-apply patterns, turn phases, verbatim sample sentences from the `turn.sentence` detector, and a noise-filtered prompt timeline. **It does not narrate.** Narration is the model's job; the script provides the facts.

There's a Claude Code skill at `.claude/skills/sessionstory/SKILL.md` that documents the full collect-then-narrate workflow. Project-level skills travel with the repo, so any agent in this directory can invoke it.

### REST API directly

The four endpoints worth memorizing:

| Endpoint | When to use |
|---|---|
| `GET /api/sessions` | List all sessions, find a session id |
| `GET /api/sessions/{id}/records` | Every record in a session — record-type, payload, timestamp, depth, parent_uuid |
| `GET /api/sessions/{id}/patterns` | Every pattern detected in a session — `eval_apply.*`, `turn.phase`, `turn.sentence`, `error.recovery`, `test.cycle` |
| `GET /api/search?q=…` | FTS5 full-text search across event content |

The data is yours, in CloudEvents 1.0, in JSON. No proprietary format, no opaque handles.

### Adjacent scripts in `scripts/`

When `sessionstory.py` doesn't expose what you need:

- `analyze_eval_apply_shape.py --session SID` — eval-apply cycle structure (cycles, terminal vs with-tools, tools per cycle)
- `analyze_turn_shapes.py SID` — distribution of distinct turn shapes and their probability classes
- `analyze_event_groups.py --session SID` — per-prompt event windows, phase distribution, common tool sequences
- `token_usage.py --session-id SID` — input / output / cache tokens and estimated cost
- `query_store.py` — direct SQL queries against the live SQLite store

### Validating your work against reality

When you change docs or architecture, run `scripts/check_docs.py` to verify the docs still match the live codebase. The validator catches drift between claims (in prose) and reality (in `Cargo.toml` workspace members, `rs/patterns/src/*.rs` files, `rs/server/src/consumers/*.rs` files, file references). It has a `--test` flag with synthetic fixtures and a Claude Code skill at `.claude/skills/check-docs/`.

### The principle

OpenStory exists so humans can see what their agents are actually doing. **Applied inward, the same principle says: give yourself visibility into what the codebase actually is, not what it claims.** If a script exists that answers your question, run it. If one doesn't, write it (and add a `--test` flag). Don't grep transcript files when there's an API. Don't assume the docs are right — verify.

---

## Architecture Principles

If you've followed the tour, you've seen these principles in action:

1. **Observe, never interfere** — read-only watcher, no mutations to the source
2. **Actor model** — independent components communicating through events/channels
3. **Functional-first** — pure transforms in the pipeline, side effects at the edges
4. **Incremental computation** — projections update per-event, never recompute
5. **BFF pattern** — server does the heavy lifting, UI receives pre-typed data
6. **Open formats** — CloudEvents, JSONL, Markdown — user-owned data
7. **BDD** — boundary tables as specs, red-green-refactor

---

## Where to Go Next

- **Add a new filter?** Start at `rs/store/src/projection.rs` (server-side) and `ui/src/lib/timeline-filters.ts` (client-side)
- **Add a new pattern detector?** Implement the `Detector` trait in `rs/patterns/src/`, register it in the patterns consumer (`rs/server/src/consumers/patterns.rs`)
- **Add a new REST endpoint?** Add handler in `rs/server/src/api.rs`, route in `build_router()` in `router.rs`
- **Add a new consumer actor?** Create a new file in `rs/server/src/consumers/`, subscribe to the right NATS subject, mirror the shape of `persist.rs` or `patterns.rs`
- **Add a new UI component?** Pure logic in `ui/src/lib/`, component in `ui/src/components/`, test in `ui/tests/lib/`
- **Understand the data?** Run `python3 scripts/sessionstory.py SESSION_ID` for a fact sheet, `scripts/query_store.py` for raw SQL, or open `scripts/explore.ipynb` (`just explore`)
- **Validate docs against the codebase?** `python3 scripts/check_docs.py` — TDD docs validator, see Stop 15 above

For the full project philosophy, read `CLAUDE.md` in the project root.
