# Architecture Audit — Event Pipeline Boundary Map

Branch: `research/architecture-audit` (off `research/hermes-integration`)

Goal: testcontainer-driven audit that sends a controlled prompt through pi-mono and asserts the event shape at every boundary. The tests become living architecture documentation.

## The pipeline, hop by hop

| # | Component | File | Input | Output | Transport |
|---|-----------|------|-------|--------|-----------|
| 1 | pi-mono agent | (external) | prompt | JSONL line | file append `~/.pi/agent/sessions/{sid}.jsonl` |
| 2 | File watcher | `rs/src/watcher.rs` | notify events + mtime | `Vec<CloudEvent>` batch | callback `(session_id, project_id, subject, Vec<CloudEvent>)` |
| 3 | Reader | `rs/core/src/reader.rs:35` | path + `TranscriptState` | `Vec<CloudEvent>` | return value |
| 4 | Translator | `rs/core/src/translate_pi.rs` | pi-mono entry `Value` | `Vec<CloudEvent>` (decomposed) | return value |
| 5 | NATS publish | `rs/src/server/mod.rs:282` | `Vec<CloudEvent>` → `IngestBatch` | JetStream msg | `events.{project}.{session}.main` |
| 6a | Persist consumer | `rs/server/src/consumers/persist.rs` | CloudEvent batch | SQLite rows + JSONL + FTS | `event_store.insert_event()` |
| 6b | Patterns consumer | `rs/server/src/consumers/patterns.rs` | CloudEvent batch | `PatternEvent` + `StructuralTurn` | publishes `patterns.{project}.{session}` |
| 6c | Projections consumer | `rs/server/src/consumers/projections.rs` | CloudEvent batch | in-memory `SessionProjection` | direct state mutation |
| 7 | SQLite | `rs/store/src/sqlite_store.rs` | serialized CloudEvent | `events` table + FTS5 | `INSERT OR IGNORE` |
| 8 | REST `/records` | `rs/server/src/api.rs:218` | session id | `Vec<Value>` (CloudEvents) | HTTP GET, reads EventStore directly |
| 9 | ViewRecord transform | `rs/views/src/from_cloud_event.rs:55` | `&CloudEvent` | `Vec<ViewRecord>` | function dispatch |
| 10 | WebSocket broadcast | `rs/server/src/consumers/broadcast.rs` + `ws.rs` | ViewRecord batch | `WireRecord` JSON | WS `Message::Text` |
| 11 | UI consumer | `ui/src/streams/connection.ts` | `WsMessage` | React state | RxJS `BehaviorSubject` |

## Audit targets (ambiguous / uncertain boundaries)

1. **Reader partial-line contract** (hop 3) — byte_offset semantics need a spec + test.
2. **Translator field normalization** (hop 4) — camelCase → snake_case at subtype layer, raw preserved. Verify views correctly branch on `agent: "pi-mono"`.
3. **NATS subject computation** (hop 5) — derived from FS path at publish time, not watch time. Must match consumer subscription filters.
4. **SQLite `data` column variance** (hop 7) — JSON blob shape varies by agent. No loss assertion needed.
5. **ViewRecord explosion for multi-tool** (hop 9) — verify multiple toolCalls in one CloudEvent produce multiple ToolCall records (pi-mono scenario 07 covers this, but container-level assertion missing).
6. **Wire enrichment in-sync with event stream** (hop 10) — `to_wire_record(vr, projection)` depends on projection freshness; verify no lag.

## First concrete audit target — syntax highlighting regression

**Observation (2026-04-14):** In Open Story UI, Read tool results show source with line numbers but no syntax highlighting (plain text). Rust `persist.rs` viewed as a ToolResult renders uncolored.

**Hypothesis:** The tool result rendering path knows the tool is `Read` (so it shows line numbers) but is either (a) not extracting language from file extension, (b) not passing language into the highlighter component, or (c) highlighter isn't mounting for ToolResult bodies (only for code blocks in AssistantMessage).

**Where to look:**
- `rs/views/src/tool_input.rs` — `ReadInput` has `path` field; is it in the output ViewRecord?
- `rs/views/src/from_cloud_event.rs` — ToolResult body construction: does it preserve file path / infer language?
- `ui/src/` — ToolResult component, check whether it routes text through the same highlighter used for assistant code blocks.

**Audit test shape:**
- Container test: send pi-mono Read of a `.rs` file, poll `/records`, assert ToolResult body carries enough metadata (path or language hint) for the UI to highlight.
- UI-level: snapshot test of ToolResult rendering a `.rs` file — expect tokenized spans, not plain text.

## Next step

Sketch container test scaffold: one compose stack (NATS leaf + hub, Open Story, pi-mono), one scripted prompt, one assertion at each hop. Start with the syntax-highlighting trace as the inaugural audit — small, concrete, visible.
