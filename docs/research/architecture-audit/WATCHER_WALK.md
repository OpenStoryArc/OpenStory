# Watcher Walk

Audit findings from `rs/src/watcher.rs`. Done as #3 in the cleanup-mode
audit pass (after the JSONL torn-line bug, the dual-write audit, the
schema registry, and the data.raw / subagent walks).

## What's in the file

Two top-level entry points:

- **`watch_with_callback(watch_dir, backfill_window_hours, on_events)`** — the production path. Does an mtime-bounded backfill, then runs a `notify`-crate event loop that fires `on_events(sid, pid, subject, batch)` for each new file event. Used by `rs/src/server/mod.rs` to feed the bus.
- **`watch_directory(watch_dir, output_file, stdout, do_backfill)`** — a CLI-mode standalone watcher that writes events to a file or stdout. Used by `rs/cli/`. Has its own backfill loop via the `backfill()` helper.

Plus three private helpers:
- `process_file_raw(path, states) -> Vec<CloudEvent>` — the actual file-read driver
- `process_file(path, states, output_file, stdout)` — wrapper that emits via `output_file` / stdout
- `backfill(watch_dir, states, output_file, stdout) -> u64` — bulk loader for `watch_directory`

## Findings

### F-1 (medium) — Two near-duplicate event loops

`watch_with_callback` and `watch_directory` both:
- Spawn `RecommendedWatcher`
- Watch the directory recursively
- Block on `rx.recv()` and match `EventKind::Modify | Create`
- Iterate `event.paths` and call `process_file_raw` / `process_file`

The differences are:
- `watch_with_callback` returns events via callback (production)
- `watch_directory` writes to file/stdout (CLI)
- `watch_with_callback` inlines the backfill loop with mtime windowing
- `watch_directory` calls a separate `backfill()` helper

A future change to one (e.g., debounce, file-system path normalization, recursion depth limit) will likely be missed in the other. This is the same drift pattern the `is_ephemeral` and `detect_subagent_relationship` audits surfaced.

**Fix shape (deferred):** extract a `WatcherLoop` struct that takes a sink (callback / writer) and runs the `notify` event loop. Both entry points become thin wrappers. Maybe 80 LOC. Not done on this branch — it's a bigger refactor than the audit appetite, and the two functions don't currently disagree, so the cost of drift is potential rather than realized.

**Why not now:** the production path (`watch_with_callback`) is rock-solid in the live integration tests and dogfood data. The CLI path (`watch_directory`) is rarely used. Refactoring the production path during the audit window risks unrelated breakage.

### F-2 (low) — Backfill window logic was inline + untestable

The mtime-window check inside `watch_with_callback` was a 6-line inline `match` expression with three branches (None / Some-and-in-window / Some-and-out-of-window). No way to unit-test the boundaries.

**Fixed this commit:** extracted to `is_in_backfill_window(window, mtime, now) -> bool`. Pure function. Four boundary tests added (None accepts everything, Some accepts within, Some rejects outside, clock-skew handled gracefully).

The fourth test is the interesting one: when `mtime > now` (clock skew, file copied with preserved mtime from a faster machine), `now.duration_since(mtime)` errs and we fall back to `Duration::ZERO`, which is "in window." The behavior was already there; the test makes it explicit.

### F-3 (low) — process_file_raw had no tests

The most-called function in the watcher had zero test coverage. Behavior was implicit: skip non-jsonl, lazy-init state per path, reuse state on subsequent calls.

**Fixed this commit:** three inline tests cover each branch.

### F-4 (informational) — symlink follow

`WalkDir::new(...).follow_links(true)` follows symlinks during backfill. An adversarial filesystem with a symlink cycle would loop. Not a current concern (default watch dirs are tame: `~/.claude/projects/`, `~/.pi/agent/sessions/`) — but worth noting if the watcher ever gets pointed at user-controlled paths.

### F-5 (informational) — no concurrent-watcher safety

`states: HashMap<PathBuf, TranscriptState>` is owned per-watcher-instance. Two watchers on the same directory in the same process would each maintain their own byte_offset and double-emit. Not a current concern (single watcher per process), but if the architecture ever grows multi-watcher support this needs revisiting.

## Tests added on this branch

- `process_file_raw_returns_empty_for_non_jsonl`
- `process_file_raw_initializes_state_on_first_call`
- `process_file_raw_reuses_state_across_calls`
- `backfill_window_none_accepts_every_file`
- `backfill_window_some_accepts_within_bound`
- `backfill_window_some_rejects_outside_bound`
- `backfill_window_handles_clock_skew_gracefully`

7 tests, all green. The watcher now has executable contracts for its two pure pieces; the remaining duplication (F-1) is documented and queued.

## Adjacent observation

`rs/src/snapshot_watcher.rs` exists separately — used for hook-style file-snapshot detection. It has its own `#[cfg(test)] mod tests` with describe-style test names ("describe_process_snapshot", "describe_wrap_message", etc.) and ~12 tests. It's well-covered. No findings.
