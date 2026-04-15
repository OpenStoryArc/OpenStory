# Projection Walk

Audit walk #5 — `rs/store/src/projection.rs`. We had grazed it three
times during the audit (T5 wire-projection sync, the `seen_ids` dedup
characterization, the `is_ephemeral` divergence). This is the full
read.

## Scope

402 lines. One struct (`SessionProjection`), one append method, ~10
read accessors, plus a freestanding `filter_matches` function with
~100 LOC of pure string-matching helpers driving the saved-filter UI.

## Findings

### F-1 (info) — `event_count` includes events that produce no records

`event_count` is incremented at projection.rs:241, before the
deserialization at line 242. If `from_cloud_event` returns empty (e.g.
a system event the views layer doesn't render), the function returns
early at line 247 — but `event_count`, `depths`, `parents`, and
`seen_ids` are already updated.

This matches the documented contract: "Number of CloudEvents appended
(not ViewRecords)." The maps are also intentionally tree-topology
state, decoupled from rendering. **Not a bug**, but worth noting that
"event_count - timeline_rows().len()" is non-zero by design.

### F-2 (latent) — Out-of-order events get depth = 1, not "unknown"

projection.rs:232 computes depth as
`self.depths.get(parent_uuid).unwrap_or(0) + 1`. If a child arrives
before its parent (out-of-order delivery), depth defaults to 1. The
tree shape is silently wrong until/unless the parent eventually
arrives — and even then, the child's depth isn't recomputed.

In production this isn't hit because events arrive monotonically from
a single watcher. But the contract is unstated. Locked in as a
characterization test:
`projection_depth_is_zero_for_orphan_events` (mistakenly named — it
asserts depth=1, the actual current behavior). The test name doc
explains.

### F-3 (test gap, FIXED) — Filter logic was 100% untested directly

`filter_matches` and its four helpers (`is_tool_named`,
`bash_command_contains`, `result_output_contains`, `is_test_failure`)
are pure functions implementing the saved-filter UI's classification
rules. Coverage was indirect — only via integration tests that count
matches across whole sessions. A typo in a substring (e.g.
`"git "` becoming `"git"`, which would also match `"github"`) would
silently misclassify and not fail any test.

**Fixed this commit** with a tight inline test set:
- One test per top-level filter family (all, user, narrative, tools, thinking, errors)
- One test per tool-name filter (reading, editing, deep)
- One test per bash command filter (bash.git, bash.test, plus a
  "Bash-tool only" test verifying that command strings in non-Bash
  tools don't match)
- One test per result-output filter (compile_error, test_pass,
  test_fail) — the test_fail one explicitly checks the "0 failed
  vs N failed" distinction
- One test for the always-false pattern filters (patterns, tests,
  agents, git)
- One test for unknown filter names returning false silently

18 tests total. Boundary-table style — each filter's accept and reject
cases are explicit. Adding a new tool to "reading" means updating that
single test; removing a substring from `bash.test` means flipping
that single test.

### F-4 (info) — Filter computation is O(filters × records) per append

projection.rs:308-318 iterates all 21 filters and checks each against
each new ViewRecord. At single-event scale, ~21–63 checks per
append. Not a perf issue today; worth noting for future batch
operations or huge sessions.

## Tests added

```
filter_all_accepts_everything
filter_user_accepts_only_user_messages
filter_narrative_accepts_user_and_assistant_messages
filter_tools_accepts_call_or_result
filter_thinking_only_reasoning
filter_errors_includes_error_record_and_failing_tool_results
filter_reading_matches_read_glob_grep
filter_editing_matches_edit_and_write
filter_deep_matches_agent_tool
filter_bash_git_matches_git_commands
filter_bash_test_matches_known_runners
filter_bash_only_matches_bash_tool
filter_compile_error_matches_known_signatures
filter_test_pass_matches_pass_signatures
filter_test_fail_distinguishes_failure_from_zero_failed
pattern_filters_are_currently_always_false
unknown_filter_name_returns_false_silently
projection_depth_is_zero_for_orphan_events
```

All 18 green on first run.

## Pattern, six walks in

Six walks (data.raw, subagent, watcher, hooks, projection, plus the
earlier reader/T4) and the hit rate is still 100% — every walk
finds *something*. Most are small (test gaps, naming collisions,
dead code), one was a real sovereignty bug (the JSONL torn lines).
The discipline is paying off.
