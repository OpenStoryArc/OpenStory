# Architecture Audit — Plan

Branch: `research/architecture-audit`
Companion: [BOUNDARY_MAP.md](./BOUNDARY_MAP.md)

## Premise

Testcontainers already proved their worth during pi-mono decomposition — they surfaced bugs that unit tests missed (projection cache staleness, case-insensitive tool names, `path` vs `file_path`). This audit extends that approach: stand up the full stack, send controlled prompts, and assert event shape at every boundary. Tests become living architecture documentation.

## Scope

**In scope:**
- NATS hub + leaf + Open Story + pi-mono running as containers
- A handful of controlled prompts that exercise each event type (text, thinking, tool_use, tool_result, multi-tool, error)
- Per-boundary assertions: shape entering, shape leaving, transport medium
- UI-adjacent assertions via WebSocket capture (no headless browser yet)

**Out of scope (for now):**
- Headless browser / full UI snapshot testing
- OpenClaw integration (readonly DB access is on backlog)
- Perf / load

## Audit targets (working queue)

Ordered roughly by visibility and blast radius — small visible wins first.

### T1. Syntax highlighting regression (Read tool results)
- **Symptom:** Read tool results render with line numbers but no token coloring.
- **Hypothesis space:**
  - a. ViewRecord ToolResult body lacks file path / language hint
  - b. UI ToolResult component uses plain `<pre>` instead of syntax highlighter
  - c. Language inference missing (`.rs` → `rust`)
- **Boundary tests:**
  - Container: pi-mono reads a `.rs` file → `/records` → assert ToolResult body carries path
  - Views unit: `from_cloud_event` for paired ToolCall+ToolResult → assert language derivable
  - UI: snapshot ToolResult with `.rs` path → expect tokenized spans
- **Fix scope:** likely one of (a) add path to ToolResult body, (b) route through existing highlighter, (c) add extension→language map.

### T2. Multi-tool ViewRecord explosion
- **Symptom:** Unverified — scenario 07 fixture exists, but no container-level assertion that two parallel toolCalls in one CloudEvent produce two distinct ToolCall ViewRecords with unique `call_id`s reaching the UI.
- **Boundary test:** Send prompt that triggers parallel reads → assert UI receives 2 ToolCall WireRecords with distinct call_ids → assert both pair correctly with their ToolResults.

### T3. NATS subject ↔ subscription alignment
- **Symptom:** Subject is derived from FS path at publish (`nats_subject_from_path`). Consumers subscribe to `events.>`. If a session has an unusual path (e.g. symlink, spaces, unicode), does the subject still match?
- **Boundary test:** Start a pi-mono session under a quirky path, assert the event arrives at all three consumers (persist, patterns, projections).

### T4. Reader partial-line contract
- **Symptom:** `TranscriptState.byte_offset` advances on successful parse; what about a half-written line during bulk flush? The watcher→reader handoff is time-sensitive.
- **Boundary test:** Simulate partial flush (write half a line, assert 0 events; write remainder, assert 1 event, no duplicates).

### T5. Wire enrichment in-sync with event stream
- **Symptom:** `to_wire_record(vr, projection)` depends on projection freshness. If projections consumer lags behind broadcast consumer (they're independent subscribers), a WireRecord might carry stale tree metadata.
- **Boundary test:** Instrument both consumers; under load, assert wire records never reference a parent_uuid the projection hasn't seen.

### T6. SQLite `data` column variance
- **Symptom:** Varies by agent payload type. No guarantee all fields round-trip.
- **Boundary test:** For each agent (Claude Code, pi-mono, Hermes), assert CloudEvent → SQLite → CloudEvent is lossless.

## Test harness shape

Proposed directory: `rs/tests/audit/` with one file per target. Shared fixtures:

- `harness.rs` — compose stack bring-up (NATS hub + leaf + Open Story), health checks, tear-down
- `pi_mono_driver.rs` — spawn pi-mono with a controlled prompt, wait for session JSONL to stabilize
- `capture.rs` — subscribe to NATS, poll REST, open WebSocket — return transcripts at each hop
- `assertions.rs` — shape-matchers (`assert_cloudevent_shape`, `assert_wire_record_shape`)

Each audit test is a narrative:
```
given: pi-mono running, prompt "read foo.rs"
when: driver sends prompt, waits for turn completion
then:
  at hop 2 (watcher): 3 CloudEvents emitted (thinking, tool_use, tool_result)
  at hop 5 (NATS): 3 messages on events.<sid>.main
  at hop 6a (SQLite): 3 rows in events table
  at hop 8 (REST): GET /records returns 3 entries, tool_use has path="foo.rs"
  at hop 9 (views): ToolResult has language="rust"
  at hop 10 (WS): 2 WireRecords (ToolCall, ToolResult) with matching call_id
```

## Execution order

1. Scaffold `rs/tests/audit/harness.rs` — container bring-up, reuse existing `helpers/container.rs` patterns
2. Write T1 (syntax highlighting) end-to-end — smallest visible target, forces every layer of the harness to work
3. Fix whatever T1 surfaces; commit
4. Layer T2 on top of the now-working harness
5. T3–T6 in whatever order the bugs demand

## Non-goals

- No refactoring during the audit. If a boundary is ugly, write the test, note it, keep moving. Refactor in a follow-up branch.
- No new event types. The audit is about verifying what's there, not adding to it.
- No UI work unless T1 forces it.

## Open questions

- Do we want pi-mono running inside a container or on the host with proxy/NATS routed through? (Host-run was simpler for research; containerized may be needed for reproducibility.)
- Should the harness reuse `testcontainers-rs` or the existing hand-rolled helpers in `rs/tests/helpers/container.rs`?
- Where does the audit output live long-term? These tests double as docs — do we generate a markdown report from test output?
