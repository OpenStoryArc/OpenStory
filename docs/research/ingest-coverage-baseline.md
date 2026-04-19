# `ingest_events` Coverage Baseline

Captured on branch `test/phase-0-audit` against `research/hermes-integration`
as the starting point for the Actor 4 decomposition refactor
(`/Users/maxglassie/.claude/plans/can-we-plan-a-cuddly-moler.md`).

## Tool

```
cargo install cargo-llvm-cov --locked
rustup component add llvm-tools-preview
cargo llvm-cov --workspace --exclude open-story-cli --ignore-run-fail --no-report
cargo llvm-cov report -p open-story-server
```

`--ignore-run-fail` is required because the following tests fail on this
branch and are independent of coverage concerns:

- `test_dedup::test_seen_ids_loaded_from_persistence` (pre-existing on
  hermes-integration; tracked in BACKLOG)
- `test_security` (a subset fails under instrumentation — needs
  investigation, but not blocking)
- `test_raw_passthrough_invariant` (pre-existing)
- `open-story-server --lib` (one or more doctest mismatches)

These failures do not invalidate the coverage measurement for
`rs/server/src/ingest.rs`, which is driven by `test_ingest.rs`,
`test_broadcast.rs`, `test_projection.rs`, `test_ws.rs`, and the inline
tests.

## Summary

| Metric | Covered | Total | % |
|---|---|---|---|
| Lines | 766 | 878 | 87.24% |
| Regions | 1184 | 1379 | 85.86% |
| Functions | 74 | 78 | 94.87% |

**Decent but not sufficient for a deletion-level refactor.** 112 lines
and 195 regions uncovered; 4 functions fully uncovered. The
uncovered regions cluster around three themes.

## Uncovered regions — characterized

### Theme 1: `replay_boot_sessions` branches (≈75% of misses)

`rs/server/src/ingest.rs:328-439` — exercised by `test_sqlite_e2e.rs`
but only on the happy path. Uncovered branches:

- **L346-348** — `if events.is_empty() { continue; }` — the "session row
  exists but no events" case. Can happen after event-dedup wiped all
  events from a session; no test covers it.
- **L371-373** — `serde_json::from_value::<CloudEvent>(val).err()` arm.
  Triggered if a row in SQLite fails to deserialize as a CloudEvent.
  Should be impossible given writer invariants, but the branch exists
  as defensive code. Worth a test that writes malformed JSON to SQLite
  and verifies replay tolerates it.
- **L377-389** — `full_payloads` capture during replay. Only fires for
  tool-result rows with `output.len() > TRUNCATION_THRESHOLD` (100KB).
  No fixture exercises this. Critical for Phase 1 because
  BroadcastConsumer will own the equivalent capture during live ingest.
- **L396-401** — FTS5 backfill-during-replay. Only fires if
  `fts_count() == 0` at boot. Needs a test that boots with events in
  SQLite but an empty FTS index.
- **L416-420** — the trailing `upsert_session` post-replay. Unreached
  when all sessions were empty (see L346) or when projections lookup
  returns None (shouldn't happen after L367).
- **L424-428** — the `fts_note` conditional in the boot log message.
  Shows up as two uncovered lines for the `fts_needs_backfill` true
  branch.

### Theme 2: Metrics reporting branches (≈10% of misses)

`rs/server/src/ingest.rs:282-316`:

- **L282** — `if !view_records.is_empty() || !append_result.filter_deltas.is_empty() || !detected_patterns.is_empty()` — the compound guard for broadcasting. Uncovered sub-branches exist when `view_records` is empty but `filter_deltas` isn't. Rare; no test covers.
- **L293** — `record_events_ingested` metric call. Only un-hit because
  `crate::metrics` module has a no-op fallback when metrics feature isn't
  compiled. Likely a false positive.
- **L296, L301, L303, L316** — metrics dedup counter, pattern counter,
  and friends. Same no-op-fallback pattern.

### Theme 3: Project-resolution fallback (≈5% of misses)

`rs/server/src/ingest.rs:49-83`:

- **L64** — exit of the project-name insertion `if` when
  `project_id` was passed in. Covered path; single-line miss is likely
  a profiler artifact.
- **L67-83** — the `cwd`-based project derivation when `project_id` is
  None. Covered by `ingest_derives_project_from_cwd_fallback` but
  branches within the `find_map` closure aren't fully covered (the
  closure for events with no `cwd` field returns None — uncovered sub-branch).

### Theme 4: Inline test assertion error paths (≈10% of misses)

Unreached `panic!`, `unreachable!`, and `expected ... got {:?}` arms in
inline tests (lines 523-553, 604, 616, 625-626, 628, 661, 670-671, 673,
725, 731, 767, 771, 901-902, 927, 1006). These are test-assertion
failure branches that only execute when tests fail — expected to be
0-hit on a green baseline. Not worth writing tests for; they *are* the
tests.

## Functions fully uncovered (4)

Running `cargo llvm-cov report -p open-story-server` against ingest.rs
reports 4 fully-uncovered functions. Most likely candidates (requires
line-level check via HTML report at `target/llvm-cov-html/html`):

- Helper functions used only by the `ingest_events` replay path we
  didn't exercise (full_payload capture helper, log formatting for
  boot replay).
- The `is_plan_event` and `extract_plan_content` public paths may be
  re-exports that aren't called from within this crate's test suite —
  worth confirming they're tested from `open-story-store` instead.

## Follow-up: tests to add before Phase 1

Plan commit 0a follow-up (task 8):

1. **`replay_with_empty_session_skips_cleanly`** — covers L346-348.
2. **`replay_tolerates_deserialization_failures`** — covers L371-373.
   Insert a malformed row into SQLite; assert replay logs but doesn't
   panic.
3. **`replay_captures_truncated_tool_result_full_payload`** — covers
   L377-389. Fixture with a tool result >100KB; assert replay populates
   `state.store.full_payloads`.
4. **`replay_backfills_fts_when_index_empty`** — covers L396-401.
   Seed SQLite with events but delete FTS rows; boot; assert FTS is
   repopulated.
5. **`ingest_broadcasts_filter_delta_only_update`** — covers L246-247.
   A batch where no view records are produced but filter deltas exist.
6. **`ingest_derives_project_when_cwd_missing`** — covers the "cwd not
   present in any event" branch in the L67-83 fallback.

Remaining uncovered branches (inline-test `panic!` arms, metrics
fallbacks) are not worth writing tests for.

## Re-run

```
cargo llvm-cov --workspace --exclude open-story-cli --ignore-run-fail --no-report
cargo llvm-cov report -p open-story-server        # summary
cargo llvm-cov report -p open-story-server --html # line-level HTML at target/llvm-cov-html/html/
```
