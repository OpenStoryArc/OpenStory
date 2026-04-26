---
slug: finish-cloudevent-typed-migration
status: in-progress
sprint: 1
---

# Finish CloudEvent::new Typed EventData Migration

## User Story
As a contributor, I want `just test` to be fully green so that CI is unblocked and new features can land safely.

## Background
A multi-week half-finished refactor tightened `CloudEvent.data` from `serde_json::Value` to typed `EventData`, plus changed several store constructor signatures (`SessionStore::new`, `EventLog::new` from `PathBuf` → `&Path` returning `Result`; `PersistConsumer::new` from 0-arg → 2-arg). The production code was updated. Most test fixtures and a few production call sites were *not*. CI has been red on every commit since at least `74cffd60`.

### Already fixed (prior work)
- `rs/views/src/from_cloud_event.rs` — `make_cloud_event` and `make_legacy_event` test helpers now return typed `CloudEvent`. New `make_event_data` helper wraps logical fixture fields into `AgentPayload::ClaudeCode` shape. Plus a real production bug fix: the single-tool typed path was hardcoding `call_id: String::new()` instead of extracting it from the raw content block.
- `rs/store/src/ingest.rs` — new `to_cloud_event` helper; 2 call sites updated.
- `rs/store/src/state.rs` — `ingest_event_into_store_state` test fixture rewritten with typed `agent_payload`.
- `rs/store/src/queries.rs` — `insert_tool_event` and `insert_error_event` SQL helpers wrap fields in `agent_payload`.
- `rs/bus/src/lib.rs` and `rs/bus/tests/nats_integration.rs` — 2 `CloudEvent::new` call sites switched from raw `Value` to `EventData::new(...)`.

## Acceptance Criteria
- [ ] `just test` is fully green (cargo test --workspace --exclude open-story-cli + npm test + clippy)
- [ ] All `CloudEvent::new` call sites use `EventData::new(...)` instead of raw `json!({...})`
- [ ] All store constructor call sites match current signatures (`SessionStore::new(&Path) -> Result`, `PersistConsumer::new(store, session_store)`)
- [ ] No regressions in existing passing crates (views, store, bus)

## Scope — Known Broken Sites

### `rs/server/src/ingest.rs`
7 sites at lines 584, 672, 730, 773, 817, 861, 902 calling `CloudEvent::new(... json!({...}) ...)` where the third arg should be `EventData::new(...)`. Mechanical fix.

### `rs/server/src/consumers/persist.rs`
6 sites around lines 130, 141–149 with the older constructor signatures (`PersistConsumer::new()` without args, `SessionStore::new(PathBuf)` instead of `&Path`, missing `.expect()` on `Result` returns). Deeper test rot — multiple constructors changed and the tests weren't updated.

### `rs/tests/`
6 integration test files (`test_consumers.rs`, `test_subject_hierarchy.rs`, `test_view_api.rs`, `test_pattern_integration.rs`, `test_ingest.rs`, `test_api.rs`, `helpers/mod.rs`) reference `CloudEvent::new` and may have similar stale call sites; status unknown until the server crate compiles.

## Fix Pattern
Every fix follows the same pattern — wrap fixture data in `AgentPayload::ClaudeCode` (with `_variant: "claude-code"` and `meta.agent: "claude-code"`), or use `EventData::new(raw, seq, session_id)` when constructing `CloudEvent::new` directly. Reference helpers:
- `make_event_data` in `rs/views/src/from_cloud_event.rs`
- `to_cloud_event` in `rs/store/src/ingest.rs`

## Edge Cases
- Ensure pi-mono fixtures (if any exist in tests) use `AgentPayload::PiMono` variant, not ClaudeCode
- Constructor signature changes may cascade — if `PersistConsumer::new` now takes args, all test setup must provide them

## Out of Scope
- New feature work — this is purely fixing test/fixture rot
- Refactoring the typed EventData design itself
- Adding new tests beyond what's needed to compile and pass

## Open Questions
- None — the fix pattern is well-documented from prior work
