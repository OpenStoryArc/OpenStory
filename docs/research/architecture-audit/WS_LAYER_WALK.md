# WebSocket Layer Walk

Audit walk #8 — `rs/server/src/ws.rs`. 201 lines. The live boundary
with the UI: `build_initial_state` is the connect-time snapshot;
`handle_socket` is the broadcast-forwarding loop.

Zero inline tests before this commit.

## Findings

### F-1 (real concern) — `Lagged` skips messages with only a log line

`ws.rs:180-183`:
```rust
Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
    log_event("ws", &format!("\x1b[33mlagged — skipped {n} messages\x1b[0m"));
    continue;
}
```

When a WS client falls behind the broadcast channel (slow network,
overloaded UI, etc.), `recv()` returns `Lagged(n)` indicating it
skipped `n` messages. We log and continue. **The UI never knows it
missed events.** Sidebar counts, timeline, token totals — all
silently diverge from server truth until a manual reload triggers a
fresh `initial_state`.

**Fix shapes** (any of):
- Send a `{kind: "lagged", skipped: n}` notification so the UI can
  refetch (cheapest)
- Close the socket, force reconnect → fresh `initial_state` (most
  honest)
- Increase channel capacity from 256 (kicks the can down the road)

The `Lagged` case can't be unit-tested without a real broadcast
channel + a slow consumer. Documenting here for follow-up.

### F-2 (UX surprise) — initial_state cap silently drops oldest records

`ws.rs:86-106`. If the projection has more records than
`max_initial_records`, the function keeps the most recent N and
**recomputes filter_counts from the capped set**. The recomputation
is correct — counts match what the UI actually sees. But there's no
indication that older records exist. From the client's perspective,
the session appears to start at the cap point.

**Fix shape:** include a `total_records` field in initial_state alongside
`records.len()`, and a `cap_applied: true` flag when truncation
happened. UI can show "showing latest N of M events" and offer a
"load more" affordance.

**Not a bug** — the cap is documented config. Just a UX/transparency
gap. Test `initial_state_caps_to_max_records_keeping_most_recent`
characterizes today's behavior.

### F-3 (test gap, FIXED) — entire WS handshake was uncovered

`build_initial_state` had zero direct tests before this commit. It's
the most-called BFF function on the server (every connect, every
reload). Its correctness drives every fresh UI render.

7 inline tests added covering:
- empty projection map → empty initial_state
- multi-session aggregation
- timestamp sort across sessions
- max_initial_records cap (kept-most-recent semantics)
- filter_counts recompute after cap
- session_labels populated from projection (label, branch, token totals)
- progress events excluded via projection.timeline_rows() contract

### F-4 (info) — silently swallowed serialization failures

`ws.rs:149` and `:175` both call `serde_json::to_string(&msg).unwrap_or_default()`.
If serde fails, sends an empty string. The UI receives `""` (parses
as missing fields → undefined JS behavior on the consumer side).

In practice serde shouldn't fail for our types — they're all
`#[derive(Serialize)]`-clean and the schema-registry dogfood
validates 21k+ real values. But the silent fallback hides any
future regression. Should at minimum log on failure.

**Fix shape:** match instead of unwrap_or_default; on Err, log and
skip the message rather than send empty.

## Tests added

```
initial_state_is_empty_for_empty_projection_map
initial_state_includes_records_from_each_session
initial_state_sorts_records_by_timestamp
initial_state_caps_to_max_records_keeping_most_recent
initial_state_recomputes_filter_counts_from_capped_set
initial_state_session_labels_carry_label_branch_and_token_totals
initial_state_excludes_progress_events_via_projection_filter
```

7 tests, all green on first run. The handshake is now executable contract.

## Pattern, eight walks in

Hit rate: 100% (every walk found something). This walk's haul:
- 1 real UX concern (Lagged silent drops — needs follow-up)
- 1 UX/transparency gap (cap without notification)
- 1 latent silent-fail (swallowed serde — likely never hit, but no test would catch it)
- 7 test-coverage adds for the most-called BFF function

Three more candidates that look high-yield:
- `rs/server/src/api.rs` — the REST surface
- `rs/store/src/analysis.rs` — analytics queries
- `rs/server/src/transcript.rs` — transcript reconstruction
