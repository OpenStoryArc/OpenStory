# T2 — Multi-Tool ViewRecord Explosion

Part of: [PLAN.md](./PLAN.md) · [BOUNDARY_MAP.md](./BOUNDARY_MAP.md)

## Symptom

When pi-mono emits multiple `toolCall` blocks in one assistant message (parallel reads, parallel greps), the translator correctly decomposes them into N separate CloudEvents with unique UUID5-derived IDs. We need to prove that N CloudEvents survive every downstream boundary as N distinct units, with pairing to their N tool results.

## Existing coverage

| Boundary | Covered? | Reference |
|----------|----------|-----------|
| Translator → CloudEvents (N events, unique IDs) | ✓ | `rs/core/src/translate_pi.rs` inline `test_decompose_multi_tool` |
| Real fixture (scenario 07) at translator | ✓ | `rs/tests/test_translate_pi.rs:242` `scenario_07_multi_tool_both_visible` |
| from_cloud_event → ViewRecord (N ToolCall records) | ✓ | `rs/tests/test_pi_mono_views_e2e.rs:273` `scenario_07_both_tools_have_records` |
| NATS publish → consumers (N messages) | ✗ | no assertion |
| SQLite (N rows) | partial — `pi_mono_exact_event_count_in_sqlite` counts total, not per-line multi-tool | |
| ToolCall ↔ ToolResult pairing for parallel calls | ✗ | `rs/views/src/pair.rs` handles it, but no parallel-case test |
| WebSocket WireRecord (N records delivered) | ✗ | no container-level assertion |
| UI render (N distinct tool cards) | ✗ | no snapshot test |

## Audit gap

Everything to the left of the NATS publish is proven. Everything to the right is assumed. A container test would catch:

- A consumer that batches N events into one row by mistake
- A wire serialization that collapses duplicate `call_id` or loses order
- A pairing bug where both tool results attach to the first tool call
- A WebSocket initial_state that only includes the first N-1 of N parallel calls (off-by-one in `max_initial_records` interaction with decomposed events)

## Test shape

Use existing fixture `rs/tests/fixtures/pi_mono/scenario_07_multi_tool.jsonl`. Extend `test_pi_mono_container.rs` with:

```rust
#[tokio::test]
async fn pi_mono_parallel_tool_calls_preserved_end_to_end() {
    // fixture has ONE assistant line with two parallel `read` toolCalls
    let server = start_open_story(&scenario_07_dir()).await;
    server.wait_for_sessions().await;

    let session_id = find_pi_session(&get_sessions(&server.base_url()).await)
        ["session_id"].as_str().unwrap();

    // Hop 7/8: SQLite via REST — N tool_use events
    let events = fetch_events(&server, session_id).await;
    let tool_uses: Vec<_> = events.iter()
        .filter(|e| e["subtype"] == "message.assistant.tool_use")
        .collect();
    assert_eq!(tool_uses.len(), 2, "parallel toolCalls should yield 2 tool_use events");

    // Unique event IDs
    let ids: HashSet<_> = tool_uses.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 2, "event IDs must be unique");

    // Unique tool_call_ids in payloads
    let call_ids: HashSet<_> = tool_uses.iter()
        .map(|e| e["data"]["tool_call_id"].as_str().unwrap())
        .collect();
    assert_eq!(call_ids.len(), 2, "tool_call_ids must be unique");

    // Hop 9: ViewRecord pairing — each tool_use pairs with its own tool_result
    let records = fetch_records(&server, session_id).await;
    let tool_calls: Vec<_> = records.iter()
        .filter(|r| r["record_type"] == "tool_call").collect();
    let tool_results: Vec<_> = records.iter()
        .filter(|r| r["record_type"] == "tool_result").collect();
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_results.len(), 2);

    // Pairing symmetry
    let call_ids_from_calls: HashSet<_> = tool_calls.iter()
        .map(|r| r["body"]["call_id"].as_str().unwrap()).collect();
    let call_ids_from_results: HashSet<_> = tool_results.iter()
        .map(|r| r["body"]["call_id"].as_str().unwrap()).collect();
    assert_eq!(call_ids_from_calls, call_ids_from_results,
        "every ToolCall must have a matching ToolResult by call_id");
}
```

## What this test doesn't cover (yet)

- **WebSocket delivery order** — REST reads SQLite, bypasses the broadcast path. Need a WS client in the harness (see T5).
- **UI render** — handled at the React layer, out of scope for Rust audit.
- **Pairing under load** — what if 10 parallel toolCalls? The fixture has 2. Parameterize if we ever see production cases > 3.

## Hypothesis

**Likely passes.** Decomposition is solid and `from_cloud_event` already uses typed payload `tool_call_id` for pi-mono (fixed during hermes-integration work). This test is a *regression guard*, not a bug hunt. It exists to encode the contract: "N parallel toolCalls in → N ToolCall records out → N ToolResult records paired."

If it fails, the failure localizes the bug to exactly one boundary — that's the point.

## Exit criteria

- Test added to `rs/tests/test_pi_mono_container.rs` and passes
- Commit on `research/architecture-audit`
- T2 section in this doc updated with outcome

## Outcome (2026-04-14)

**Test added** at `rs/tests/test_pi_mono_container.rs::pi_mono_parallel_tool_calls_preserved_end_to_end`. Passes against a freshly built `open-story:test` image.

**What the audit surfaced during the bug hunt:**

1. First failure was cosmetic but informative: `WireRecord` serializes `ViewRecord.body` as `payload`, not `body`. The test was checking the wrong key. Fix: one-word change. Still — a test that reads the wire format should match what the UI sees.

2. Second failure was the real catch: **the container was running a stale binary**. The test found 4 tool_call records instead of 2. Local deserialization (same event, same `from_cloud_event` code) returned 1 record per event. The container returned 2. Diff = the 22-hour-old `open-story:test` Docker image predated commit `5eff284 fix(views): use typed payload for pi-mono decomposed tool calls`.

**Lessons for the audit doctrine:**

- **Rebuild-before-test is non-negotiable.** Container tests against a stale image are worse than no test — they pass on bad code and fail on good code. A reasonable follow-up is a Makefile/justfile target that combines `docker build` + `cargo test --test test_pi_mono_container` so the two can't drift. See BACKLOG.md: "Testcontainer Improvements" is related but specifically about shared-container patterns; add a sub-item about freshness.

- **Diagnostic-first debugging paid off.** Adding `eprintln!` with full record + deserialization results pointed to the exact divergence (container said 2, local said 1) in one re-run. Without that, we would have gone hunting through `extract_tool_calls` and `from_cloud_event` branches for a bug that wasn't there.

- **The test itself is the boundary spec.** It encodes: "N parallel toolCalls → N distinct CloudEvents → N unique call_ids → N ToolCall records → N ToolResult records → 1-to-1 pairing." Any future regression at any of those boundaries now fails this test.

**Outcome summary:** test passes, bug was stale image not stale code, audit earned its keep.
