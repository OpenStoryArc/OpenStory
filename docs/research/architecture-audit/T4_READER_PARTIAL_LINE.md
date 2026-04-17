# T4 — Reader Partial-Line Contract

Part of: [PLAN.md](./PLAN.md) · [BOUNDARY_MAP.md](./BOUNDARY_MAP.md)

## Why this boundary matters

`read_new_lines()` is the first reader of agent-written JSONL. The whole pipeline depends on one contract:

> A line is considered complete only when it ends with `\n`.
> The byte offset advances only past complete lines.
> A re-read with a partially-flushed line returns zero events.
> The same re-read after the line is flushed returns one event.

Pi-mono bulk-flushes (entire session content at once), Claude Code streams line-by-line — both rely on this contract. If the reader ever advances past an incomplete line, the partial content is either parsed as garbage (skipped, but offset advanced → next complete line never revisited in context) or deserialized into a truncated CloudEvent. Either way, data is lost silently.

## Recon

Current implementation (`rs/core/src/reader.rs`):

```rust
if !line_buf.ends_with('\n') {
    break;  // do NOT advance byte_offset
}
state.byte_offset += bytes_read as u64;  // only on \n-terminated line
```

The logic is right. The bug surface is that it's only implicitly tested — no unit tests in `reader.rs` exercise the partial-line edge. The integration tests all write complete JSONL fixtures; none simulate a torn write.

## Test shape

Four inline unit tests in `reader.rs`:

1. `read_returns_no_events_on_partial_line` — write `{"type":"message"` with no `\n`, expect 0 events, offset unchanged
2. `read_picks_up_after_partial_completes` — append the rest + `\n`, expect 1 event, offset advanced past full line
3. `read_advances_past_invalid_json_but_complete_lines` — write an invalid JSON line terminated by `\n`, expect 0 events, offset advanced (so the line is not re-read)
4. `read_handles_mixed_complete_and_partial` — write `line1\nline2(no newline)`, expect 1 event and offset at the end of line1

These nail the contract: "completeness is keyed on `\n`, nothing else."

## Hypothesis

All four pass against current code. T4 is a lock-in, not a bug hunt — no existing user-facing regression, just the highest-risk unguarded boundary. Worth paying the tiny cost now so a future refactor (async file I/O, tokio::io::BufReader, streaming parser) can't silently violate the contract.

## Exit criteria

- Tests inline in `rs/core/src/reader.rs`
- All pass
- T4 section updated with outcome

## Outcome (2026-04-14)

All 4 tests pass on current code. The contract is correct; it's now locked in.

Surprise — the test for invalid-but-complete JSON was arguably the most important to add. The current behavior is "advance past bad lines" (so the pipeline makes forward progress instead of stalling on one corrupt line forever), but nothing before this commit encoded that expectation. A well-meaning future change to "retry on parse failure" would regress that into an infinite loop; now it fails this test first.

**Audit value:** the reader is the most upstream boundary. Any drift here propagates everywhere. Four cheap tests, no bug, but the contract is now executable.