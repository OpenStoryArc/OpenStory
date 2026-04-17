# Audit Harness — Sketch

Part of: [PLAN.md](./PLAN.md)

## Layout

```
rs/tests/audit/
├── mod.rs                  # re-exports
├── harness.rs              # stack bring-up / tear-down
├── pi_mono_driver.rs       # drive pi-mono with prompts
├── capture.rs              # NATS/REST/WS capture
├── assertions.rs           # shape matchers
├── fixtures/
│   └── hello.rs            # known-content files for tool calls
└── t1_syntax_highlighting.rs
└── t2_multi_tool.rs
└── ...
```

## `harness.rs` — stack bring-up

Reuses patterns from `rs/tests/helpers/container.rs` (already proven in pi-mono container tests) but adds NATS leaf + hub topology.

```rust
pub struct AuditStack {
    pub nats_hub: Container,       // nats:2.10 w/ jetstream
    pub nats_leaf: Container,      // leaf pointing at hub
    pub open_story: Container,     // openstory server connected to leaf
    pub work_dir: TempDir,         // pi-mono session dir
    pub api_url: String,           // http://localhost:<port>
    pub ws_url: String,            // ws://localhost:<port>/ws
    pub nats_url: String,          // nats://localhost:<port>
}

impl AuditStack {
    pub async fn start() -> Self { ... }
    pub async fn stop(self) { ... }
    pub fn session_dir(&self) -> &Path { &self.work_dir }
}
```

Bring-up order: hub → leaf (must connect) → open-story (must see NATS) → health probe `/api/sessions`.

## `pi_mono_driver.rs` — controlled prompts

Two modes considered:
- **A. Spawn pi-mono as a subprocess**, pipe prompt via stdin, let it write JSONL to `session_dir`.
- **B. Synthesize JSONL directly** (bypass LLM call), inject pre-captured events into the session file.

Mode B is faster, deterministic, and cheaper (no API costs). Mode A is more honest but flaky. **Start with B**, using captures from `docs/research/pi-mono-integration/captures/`. Add A later for the "true e2e" tests.

```rust
pub struct PiMonoDriver<'a> {
    stack: &'a AuditStack,
    session_id: String,
}

impl PiMonoDriver<'_> {
    pub async fn inject_jsonl(&self, fixture: &str) { /* write fixture to session_dir */ }
    pub async fn wait_for_turn_end(&self) { /* poll REST for turn_end record */ }
}
```

## `capture.rs` — multi-layer taps

```rust
pub struct Capture<'a> {
    stack: &'a AuditStack,
    nats_sub: nats::Subscription,  // events.>
    ws_client: WebSocketClient,    // connected early so no messages missed
}

impl Capture<'_> {
    pub async fn attach(stack: &AuditStack) -> Self { ... }
    pub async fn nats_events(&self) -> Vec<CloudEvent> { /* drain sub */ }
    pub async fn rest_records(&self, session: &str) -> Vec<Value> { /* GET /records */ }
    pub async fn ws_records(&self) -> Vec<WireRecord> { /* drain ws queue */ }
    pub async fn sqlite_rows(&self, session: &str) -> Vec<Row> { /* direct query, optional */ }
}
```

Attach **before** driver.inject_jsonl to avoid missing early events.

## `assertions.rs` — shape matchers

Keep matchers small and named after what they assert, not how. The test body reads like prose.

```rust
pub fn assert_has_subtype(events: &[CloudEvent], subtype: &str) -> &CloudEvent { ... }
pub fn assert_tool_call(records: &[ViewRecord], name: &str) -> &ToolCall { ... }
pub fn assert_paired_tool_result(wire: &[WireRecord], call_id: &str) -> &ToolResult { ... }
pub fn language_from_path(path: &str) -> Option<&'static str> { ... } // rs→rust, etc.
```

## Reuse vs new

| Concern | Reuse | New |
|---------|-------|-----|
| Container orchestration | `rs/tests/helpers/container.rs` | leaf + hub topology |
| Fixtures | `rs/tests/fixtures/pi_mono/` | + `audit/fixtures/hello.rs` |
| CloudEvent types | `rs/core` / `rs/views` | — |
| REST client | existing reqwest in helpers | — |
| NATS client | `async-nats` (already a dep) | subscription wrapper |
| WS client | — | thin tungstenite wrapper |

## CI posture

Audit tests are slow (container start-up). Gate behind `--ignored` or a feature flag `--features=audit` so `cargo test` default stays fast. Explicit command: `cargo test --test audit --features audit -- --ignored --nocapture`.

## Narrative logging

Each audit test prints a per-hop log line to stdout so the test output reads like a trace:

```
[T1] hop 1  pi-mono    → wrote 3 JSONL lines (120 bytes)
[T1] hop 5  NATS       → 3 events on events.test-sid.main
[T1] hop 7  SQLite     → 3 rows in events
[T1] hop 8  REST       → /records returned 3 entries
[T1] hop 9  views      → ToolResult body has path? NO ← FAIL
```

The `FAIL` line tells you exactly which hop dropped the information. That's the whole point.
