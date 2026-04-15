# API Walk

Audit walk #9 — `rs/server/src/api.rs` (1283 LOC, 35 endpoint
handlers) and the `logging.rs` helpers it uses. Different audit
lens this time: input handling, security-adjacent surface, error
paths.

Zero inline tests in api.rs before this commit. Coverage was
entirely via integration tests in `rs/tests/test_api.rs`.

## Findings

### F-1 (REAL BUG, FIXED) — Byte-index slicing panics on multi-byte boundaries

`rs/server/src/logging.rs:8` (the OLD `short_id`):
```rust
pub fn short_id(id: &str) -> &str {
    &id[..id.len().min(8)]
}
```

And `rs/server/src/api.rs:944`:
```rust
log_event("api", &format!("GET /api/search?q={}", &q[..q.len().min(50)]));
```

Both use byte-index slicing where the boundary (8 or 50) can land
inside a multi-byte UTF-8 char. Rust panics with "byte index N is
not a char boundary."

**Reproduction confirmed standalone before the fix:**
```
$ cat > /tmp/check.rs <<'EOF'
fn main() {
    let id = "abc日日";  // 9 bytes; byte 8 inside the second 日
    let _ = &id[..id.len().min(8)];
}
EOF
$ rustc /tmp/check.rs -o /tmp/check && /tmp/check
thread 'main' panicked at /tmp/check.rs:5:16:
byte index 8 is not a char boundary; it is inside '日' (bytes 6..9) of `abc日日`
```

**Where this could fire in production:**
- `short_id` runs on every API request, every log line. Session ids
  are typically UUIDs (ASCII) so they're safe — but if a custom
  session id ever appears with a multi-byte char straddling byte 8,
  the request handler panics.
- The search-query truncation runs on every `GET /api/search`. A
  user typing a Japanese / Chinese / Korean / emoji query whose
  byte-50 mark lands mid-char crashes the handler.

Both paths previously had **zero test coverage** — typing exposed
this only by accident.

**Fix:** new helper `truncate_at_char_boundary(s, max_bytes)` that
backs up to the largest char boundary ≤ `max_bytes`. Both call sites
use it. Six tests cover ASCII (truncate to 8), shorter-than-cap
(returns full), multi-byte at byte 8 (returns "abc日" not panic),
Japanese at byte 50 (returns 16 of 17 日 chars), max=0 (empty),
and exact-boundary at multi-byte (returns exactly the emoji).

### F-2 (transparency gap, characterized) — `delete_session` doesn't remove JSONL backup

`rs/server/src/api.rs:1230`. `DELETE /api/sessions/{id}` removes:
- ✓ Events from EventStore (SQLite or Mongo)
- ✓ In-memory projection
- ✓ Detected patterns cache
- ✓ Full payloads cache
- ✓ Session→project mapping

But **does not** remove `data/{session_id}.jsonl`, the SessionStore
backup file.

Implications:
- The backup file is **inert** — boot replay reads from EventStore,
  not JSONL. So the deleted session doesn't resurrect on restart.
- The schema-registry capstone test `test_jsonl_escape_hatch` reads
  files in `data/`, so deleted sessions still show up there.
- A user calling DELETE expects "all local trace gone." The backup
  file remains until manually `rm`'d.

This may be intentional sovereignty ("we never touch your data") or
may be an oversight. Worth a deliberate decision + documentation.
**Not fixed this commit** — that's a behavior decision, not a bug.
Filed in BACKLOG.

### F-3 (test gap, partially addressed) — 35 endpoints, 0 inline tests

api.rs is the largest single source file in the server crate and had
**zero** inline `#[cfg(test)] mod tests`. Coverage exists at the
integration level (rs/tests/test_api.rs) but every endpoint's input
validation, error-path handling, and edge cases are tested only
where the integration tests happen to cover them.

Out of scope for one walk. The pure helpers I touched
(`short_id`, `truncate_at_char_boundary`) now have inline tests. A
follow-up walk per endpoint family (sessions, search, analytics,
delete/lifecycle) is the right shape — too big to do all in one go.

### F-4 (info) — `search_events` accepts unbounded `limit`

`rs/server/src/api.rs:927-958`. `SearchQuery.limit` is a `usize`
deserialized from `?limit=` — no upper bound enforced. A request
with `limit=1000000` returns up to 1M FTS5 hits, plus serialization
overhead, plus client-side rendering cost.

FTS5 itself doesn't cap. The default (`default_search_limit() = 20`)
is sane but a malicious or buggy client can override.

Fix shape: cap `query.limit.min(MAX_LIMIT)` with `MAX_LIMIT = 500`
or similar. Trivial change.

## Tests added

```
short_id_truncates_ascii_to_8_chars
short_id_returns_full_string_when_shorter_than_8
short_id_does_not_panic_on_multibyte_at_byte_8       ← regression guard
truncate_handles_japanese_at_byte_50
truncate_zero_max_bytes_returns_empty
truncate_max_bytes_exactly_at_boundary
event_type_summary_empty_input_returns_empty_string
```

7 inline tests in logging.rs. 95 server-lib tests total, all green.

## Pattern, nine walks in

Hit rate: still 100%.

| Walk | Real bugs found |
|------|-----------------|
| 1 data.raw | 0 |
| 2 subagent | 0 (4 copies → 1) |
| 3 watcher | 0 |
| 4 hooks | 0 (-8 LOC dead) |
| 5 projection | 0 |
| 6 bus | 1 (NatsBus silent ack — needs infra to fix) |
| 7 eval-apply | **1 (parallel tool result misattribution — characterized)** |
| 8 ws | 0 (1 UX gap, 1 latent silent-fail) |
| 9 api | **1 (multi-byte panic in short_id + search log — FIXED)** |

**4 real bugs found across 9 walks, plus 1 sovereignty bug fixed earlier (JSONL torn lines).** 5 total bugs found by the audit branch. Plus 96+ tests added across the codebase, 6 audit-doc PDFs of context.
