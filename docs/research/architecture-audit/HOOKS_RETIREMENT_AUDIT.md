# Hooks Retirement — Cleanup Audit

Walk #4. CLAUDE.md and a referenced commit (`e17ae27 — kill /hooks
endpoint + retire seen_event_ids dedup machinery`) say the `/hooks`
HTTP endpoint was retired. Audit: did the cleanup actually finish?

## What's gone

- `rs/server/src/hooks.rs` — file does not exist on disk
- No `pub mod hooks`, no `use crate::hooks` anywhere in the source tree
- The `/hooks` route is not registered in either router (publisher or full)

The endpoint and its handler module are properly gone. ✓

## What was left dangling

Three pieces of vestigial code that referenced the retired endpoint:

### 1. Misleading test name (`router.rs:283`)

```rust
async fn publisher_router_has_hooks_and_health() {
    // Tests /health and /api/sessions — no hooks check anywhere
}
```

The test body verifies that the publisher router serves `/health`
(200) and rejects `/api/sessions` (404). There is no hooks assertion
because there is no hooks endpoint. The name is from before the
retirement.

**Renamed:** `publisher_router_serves_health_only_after_hooks_retirement`.

### 2. Dead metrics (`metrics.rs`)

Three constants registered, one function defined, **zero call sites**
in production code. All four were exercised only by their own
self-tests:

- `names::HOOKS_RECEIVED` — counter name for hook receipts
- `names::HOOK_DURATION` — histogram name for hook latency
- `names::INGEST_DURATION` — histogram name for ingest latency
- `pub fn record_hook_received()` — counter increment helper

`HOOKS_RECEIVED` is leftover from the endpoint. `HOOK_DURATION` and
`INGEST_DURATION` are histogram names — but a grep for `histogram!`
across the codebase returns zero hits. No `histogram!()` callsite
ever existed. The names were registered in anticipation of latency
tracking that never landed.

**Removed:** all four. The metric-name validation test and the
no-panic test updated to reflect the smaller set. Net: -8 lines,
no behavior change (the dead pieces were never wired in).

### 3. Stale Role doc comment (`config.rs:11`)

```rust
/// - `Publisher`: watcher + hooks server, publishes to NATS, no local store
```

`Publisher` mode no longer runs a hooks server. Today it's just
the watcher + a `/health` endpoint.

**Updated:** comment now says watcher only, and explicitly notes
that the `/hooks` endpoint was retired.

## What's intentional, not vestigial

A handful of `hooks` references are correct and should stay:

- `rs/tests/fixtures/synth_hooks.jsonl` and friends — test fixtures
  containing `subtype: "system.hook"` events. The `system.hook`
  event subtype itself still exists; it's emitted by Claude Code
  hooks (configured externally via `~/.claude/settings.json`) and
  arrives via the file watcher path, not the retired HTTP endpoint.
- `rs/server/src/transcript.rs:228` — accepts `arc://hooks/{id}`
  source URIs as a legacy format. Receiving them shouldn't crash
  even though we don't produce them anymore.
- `consumers/persist.rs:13` doc comment — historical narrative
  ("retired alongside the /hooks endpoint that needed it"). Useful
  context for readers, kept.
- `consumers/patterns.rs:70` — `should_skip_pattern_detection`
  excludes `system.hook` from eval-apply. Still relevant: hook
  events arrive, get persisted, but don't feed pattern detection.

## Pattern across audit walks

Every walk through a previously-untouched module finds either:
- A naming collision (is_ephemeral × 3),
- Duplicated logic (subagent detection × 4),
- Dead code (hooks metrics × 4), or
- Untested pure functions (backfill window, process_file_raw).

Not surprising — codebases drift naturally during refactors. The
discipline of "read the file with audit eyes, write tests for
the contracts" turns the drift into commits.

## Tests / commits

- `publisher_router_serves_health_only_after_hooks_retirement` — renamed
- `metrics::tests::metric_names_are_prometheus_valid` — narrower set
- `metrics::tests::record_functions_do_not_panic_without_recorder` — `record_hook_received` removed from the call list

Build: clean. Tests: 81 server lib tests pass.
