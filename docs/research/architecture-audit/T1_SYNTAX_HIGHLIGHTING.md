# T1 — Syntax Highlighting Regression

Part of: [PLAN.md](./PLAN.md) · [BOUNDARY_MAP.md](./BOUNDARY_MAP.md)

## Symptom

Open Story UI renders pi-mono Read tool results with line numbers but no token coloring. Rust source appears as monospace plain text. Observed 2026-04-14 on `research/hermes-integration` against a pi-mono session reading `persist.rs`.

## Why this is the inaugural audit target

- Visible — confirmable in the UI without instrumentation
- Small — the failure is at one hop (unknown which) but the pipeline is short
- Cross-cutting — forces the harness to exercise every layer: pi-mono driver, NATS capture, REST, views, WebSocket, UI
- Representative — if we can trace this and fix it, the pattern scales to T2–T6

## Trace plan

At each hop, capture the shape and assert the expected presence of **file path** and/or **language hint**.

| Hop | Capture | Expected shape | Current (hypothesized) |
|-----|---------|----------------|------------------------|
| 1 pi-mono JSONL | grep session file | `toolCall.arguments.path = "persist.rs"` | ✓ present |
| 2 watcher → 4 translator | watcher callback intercept | CloudEvent `message.assistant.tool_use` with `payload.args.path` | ✓ present (pi-mono decomposition verified) |
| 5 NATS publish | subscribe `events.>` | same CloudEvent | ✓ present |
| 7 SQLite | SELECT from events | `data` blob contains path | ✓ present (via `/records` response) |
| 8 REST `/records` | GET, assert | ToolCall entry has `input.path = "persist.rs"` | ✓ (verified manually via UI showing line numbers → Read renderer is finding *something*) |
| 9 ViewRecord (ToolResult) | `from_cloud_event` output | ToolResult body has path OR language field | ❓ **prime suspect** |
| 10 WireRecord (WS) | capture ws frame | WireRecord carries language hint | ❓ downstream of hop 9 |
| 11 UI ToolResult component | render inspect | wraps result text in syntax highlighter with `language="rust"` | ❓ **secondary suspect** |

## Three hypotheses

### H1: ToolResult ViewRecord doesn't carry path/language
Tool call and tool result are paired by `call_id`, but the result body may be just `{content: "..."}` with no back-reference to the tool or its input. If so, language can't be inferred downstream.

**Check:** `rs/views/src/from_cloud_event.rs` — the ToolResult branch construction. Does it include the tool name or path?

**Fix if true:** Thread the path (or derived language) from ToolCall into ToolResult at pairing time, OR have the UI look up the matching ToolCall by `call_id` and extract path from its input.

### H2: UI ToolResult component uses plain `<pre>` for line-numbered output
The Read result has line numbers because some component adds them. That component might be a dedicated "numbered text" renderer that never hands off to the syntax highlighter used for assistant code fences.

**Check:** `ui/src/` — find the component that renders ToolResults with line numbers. Compare to the component that renders assistant message code blocks.

**Fix if true:** Route numbered text through the same highlighter, passing language as a prop.

### H3: Language inference gap
Even if path makes it to the UI, there may be no `.rs → rust` mapping in scope. The highlighter (likely Shiki, Prism, or highlight.js) needs a language name, not an extension.

**Check:** `ui/src/` — search for extension maps or language detection helpers.

**Fix if true:** Add a small `ext → lang` map (rs→rust, py→python, ts→typescript, etc.) applied at render time.

## Likely outcome

My prior is H2 (UI) with a dash of H3 (missing ext map). The views layer was heavily audited during pi-mono decomposition and typed payloads are now working (we know `tool_input.rs` produces typed `ReadInput { path }` with the serde alias fix). The view data is probably fine; the UI probably isn't asking for it.

But — that's a guess. The audit test is what proves it.

## Test skeleton

```rust
// rs/tests/audit/t1_syntax_highlighting.rs

use crate::audit::harness::AuditStack;
use crate::audit::pi_mono_driver::PiMonoDriver;
use crate::audit::capture::Capture;

#[tokio::test]
async fn t1_read_rust_file_carries_language_to_ui() {
    let stack = AuditStack::start().await;
    let driver = PiMonoDriver::new(&stack).await;
    let capture = Capture::attach(&stack).await;

    // Controlled prompt: read a known .rs file
    driver.send_prompt("Read /fixtures/hello.rs").await;
    driver.wait_for_turn_end().await;

    // Hop 5: NATS
    let events = capture.nats_events().await;
    let tool_use = events.iter().find(|e| e.subtype == "message.assistant.tool_use").unwrap();
    assert_eq!(tool_use.payload_path(), Some("hello.rs"));

    // Hop 8: REST
    let records = capture.rest_records().await;
    let tool_call = records.iter().find(|r| r.body_kind() == "tool_call").unwrap();
    assert_eq!(tool_call.input_path(), Some("hello.rs"));

    // Hop 9/10: WebSocket
    let wire = capture.ws_records().await;
    let tool_result = wire.iter().find(|w| w.body_kind() == "tool_result").unwrap();

    // THE KEY ASSERTION
    assert_eq!(
        tool_result.language_hint(),
        Some("rust"),
        "ToolResult should carry language hint derivable from paired ToolCall.input.path"
    );
}
```

The assertion `tool_result.language_hint() == Some("rust")` is the one that fails today (presumed). Whether the fix is in views (enrich the ToolResult body) or UI (look up by call_id) falls out of chasing it.

## Out of scope for T1

- Other extensions beyond `.rs` — once one works, a parameterized test can cover the rest (T1b)
- Non-Read tools (Grep, Glob results) — T1c
- Inline code blocks in assistant text — already works, not regressing

## Exit criteria

- Test above passes
- Manual UI check: reading `persist.rs` shows Rust syntax highlighting
- `docs/research/architecture-audit/T1_SYNTAX_HIGHLIGHTING.md` updated with outcome (which hypothesis was right, what the fix was, what was surprising)
- Commit on `research/architecture-audit`
