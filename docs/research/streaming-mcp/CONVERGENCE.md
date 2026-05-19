# Streaming MCP — convergence on `open-story-bus`

This doc records the second iteration of the streaming MCP work: the
test-fixture split, the `Subscribe` trait, and the testcontainer suite.
It joins `MOTIVATION.md` / `PLAN.md` / `TESTS.md` as the design garden
for `rs/mcp/`.

## What was wrong with the first cut

The first iteration (PR #55, initial commits) shipped a working
streaming MCP but with three smells, all variations of the same theme —
**parallel implementations of things the workspace already had**:

1. **Bus duplication.** `rs/mcp/src/nats_bus.rs` reached directly into
   `async-nats`, reinventing connection setup. `rs/bus/` already had
   `open-story-bus::NatsBus` with JetStream semantics, token auth,
   retention, and acks. My version used **core NATS** subscriptions
   (worked because JetStream sits on top of core NATS), but gained
   nothing from the parallel path.
2. **Test fixture in production source.** `InMemoryBus` lived in
   `rs/mcp/src/bus.rs` and showed up in `AnyBus::InMemory`. The binary's
   "if NATS unreachable, use InMemoryBus" fallback was *functionally
   dead* — the in-memory bus has no publisher in production, so
   subscribers hung forever pretending to work. The `AnyBus` enum
   existed only to support this dead fallback.
3. **No real-bus integration test.** All MCP tests ran against
   `InMemoryBus`. There was a `nats_smoke.rs` for opt-in real-NATS, but
   no automated test asserting "the binary, spawned as a child process,
   subscribed via real JetStream, receives the events a publisher
   produces."

## What changed

```
┌────────────────────────────────────────────────────────────────────┐
│ BEFORE                                                             │
└────────────────────────────────────────────────────────────────────┘

  agent (MCP client)
       │ JSON-RPC stdio
       ▼
  rs/mcp/
    stdio.rs ──▶ AnyBus (enum)
                    ├─ InMemory(InMemoryBus)   ← prod fallback + test
                    └─ Nats(NatsBus)            ← direct async-nats
                              │
                              ▼
                       core NATS subscribe
                              │
                              ▼
                       NATS broker (JetStream events)
                              ▲
                              │ JetStream publish
                       rs/bus + rs/server


┌────────────────────────────────────────────────────────────────────┐
│ AFTER                                                              │
└────────────────────────────────────────────────────────────────────┘

  agent (MCP client)
       │ JSON-RPC stdio
       ▼
  rs/mcp/
    stdio.rs::run<S: Subscribe> ──▶ subscriber: S
                                        │
                                        ▼
                                  Subscribe::subscribe(sid)
                                        │
                       ┌────────────────┴────────────────┐
                       │                                 │
                       ▼ (prod)                          ▼ (tests)
                NatsBus (thin wrapper)            LoopbackSubscriber
                       │                          (tests/common/)
                       │ open_story_bus::Bus              │
                       ▼                                  │
              open_story_bus::NatsBus                     │
                       │                                  │
                       │ JetStream push consumer          │
                       │ DeliverPolicy::New                │
                       ▼                                  │
                NATS JetStream                            │
                       ▲                                  │
                       │                                  │
              rs/bus + rs/server                          │
                                                          │
                                              tests publish in-process
```

## The `Subscribe` trait

MCP's transport layer needs exactly one thing from a bus: "given a
session id, give me a `Subscription` that delivers events." Naming
that contract gives us a clean test seam:

```rust
#[async_trait]
pub trait Subscribe: Clone + Send + Sync + 'static {
    async fn subscribe(&self, session_id: &str) -> Result<Subscription>;
}
```

Production passes `NatsBus`. Tests pass `LoopbackSubscriber`. Both
deliver `Subscription` instances that wrap the *same*
`pump_subscription` function — the only bit of subscription mechanics
that belongs to MCP rather than to the bus (it adds the per-subscription
`seq` counter and adapts `IngestBatch` → `StreamEvent`).

The `AnyBus` enum is gone. So is `InMemoryBus`. So is the binary's
in-memory fallback.

## Fail-loud production policy

The binary now refuses to start if NATS is unreachable:

```rust
let subscriber = NatsBus::connect(&url).await.with_context(|| {
    format!("open-story-mcp requires NATS at {url} — is the OpenStory server running?")
})?;
```

The previous fallback pretended to work — JSON-RPC handshake succeeded,
`subscribe_session` returned a `stream_id`, but no events ever arrived
because nothing was publishing to the in-memory bus. The failure mode
was indistinguishable from "wrong session id" or "no activity." Loud
beats silent, every time.

## Test pyramid (after convergence)

```
                                          ┌──────────────────────────────┐
                                          │ slow, real-bus, real-binary  │
        ┌─────────────────────────────────┤ testcontainer_nats.rs (6)    │
        │ Docker + nats:2.10 + JetStream  │ • round-trip                 │
        │                                 │ • subagent wildcard          │
        │                                 │ • monotonic seq              │
        │                                 │ • 100×100 fan-out            │
        │                                 │ • subscribe_tokens e2e       │
        │                                 │ • binary as child process    │
        ├─────────────────────────────────┴─────────────────────────────┤
        │ nats_smoke.rs (2 — opt-in against running NATS)              │
        ├───────────────────────────────────────────────────────────────┤
        │ tests/{streaming,tokens,stdio}.rs                            │
        │ integration via LoopbackSubscriber                           │
        │ (7 tests)                                                    │
        ├───────────────────────────────────────────────────────────────┤
        │ tests/{jsonrpc,pump}.rs                                       │
        │ unit-y: pure protocol, pure pump_subscription, aggregator    │
        │ (12 tests)                                                   │
        └───────────────────────────────────────────────────────────────┘

  Each layer asserts only what it can honestly assert.
  No test theaters: bus mechanics (fan-out, durability) are asserted
  against the real bus; MCP mechanics (seq, cancel-on-drop, the
  IngestBatch→StreamEvent transform) are asserted in isolation.
```

**Total: 31 tests across 7 files. Up from 27 in PR #55's first cut.**

## Why NATS-only container (not full open-story container)

The MCP layer's job is "subscribe to a session's events on the bus."
That doesn't require the watcher, the translator, or any of the
OpenStory server's consumers. A `nats:2.10` container with `-js` is
~1s startup; the full `open-story:test` image takes ~30s to build and
boots a stack that doesn't help MCP's tests.

If we ever want to test "watcher → translator → JetStream → MCP" as
one end-to-end probe, that lives in `rs/tests/` against the full
container. Out of scope for this PR.

## Subject convention coupling — open thread

`NatsBus::subscribe` uses the wildcard `events.*.{session_id}.>`,
matching today's `events.{project}.{session}.{main|agent.id}`
convention (from `open_story_core::paths::nats_subject_from_path`).

The cybersecurity spike (`docs/research/cybersecurity-spike.md`)
proposes reshaping subjects to
`events.{person_id}.{principal_id}.{project}.{session}.>` — five
tokens instead of three between `events.` and `{session}.>`. A
single `*` won't match.

When that lands, the wildcard here needs to update. The right longer-
term answer is to pull subject construction into a typed helper in
`open_story_bus` (or `open_story_core::paths`) and call it from MCP —
so the subject convention is owned in one place. Comment in
`rs/mcp/src/nats_bus.rs` flags the line.

## What was deliberately not done

- **Reconnect-with-history.** `DeliverPolicy::New` means an agent
  that disconnects and reconnects starts fresh. Replay-from-seq is a
  ~30-line extension to `open_story_bus::Bus::subscribe` (add a
  `from: SubscribePosition` arg). Not in this PR; the streaming MCP's
  "live observe" semantics don't require it yet.
- **Auth on the stdio transport.** Stdio is implicitly trusted. When
  the remote MCP transport lands (HTTP+SSE or Streamable HTTP), it'll
  need a bearer-token layer.
- **Promoting `rs/mcp/` into the main workspace.** The crate still
  declares its own `[workspace]` so it can incubate without churning
  the main rs/ workspace. Promotion is a separate decision — wait
  until the surface stabilizes, then add to `rs/Cargo.toml::members`
  and remove the inline workspace block.

## Decision log

- **Hand-rolled bus → wrap `open-story-bus`.** Picked the wrap. Smaller
  diff in MCP, JetStream semantics for free, single subject convention
  in the workspace. Cost: one cross-workspace path dep, accepted.
- **`#[cfg(test)]` gate vs move to `tests/common/`.** Picked move. Test
  fixtures in `src/` show up in `cargo doc`, IDE autocomplete, and
  the public API surface — Rust's `#[cfg(test)]` only hides at build
  time, not at the contributor's mental-model level. The
  `tests/common/` directory is the canonical Rust home for test
  scaffolding.
- **`AnyBus` enum vs trait.** Killed the enum. With InMemoryBus gone
  from production, the enum was a one-variant masquerade. A generic
  `S: Subscribe` is honest and lets tests inject their own impl
  without intermediation.
- **6 testcontainer tests vs 7.** Dropped the "drop tears down
  consumer" test from the planned 7 — its core assertion ("CancelGuard
  fires + pump aborts") is already covered in `pump.rs` at the unit
  level. The extra container spin-up wasn't earning its keep.
- **Container choice: `nats:2.10` + `-js` flag.** Lighter than the
  full `open-story:test` image (1s startup vs 30s build), covers
  everything MCP's tests need. End-to-end probes against the full
  stack live elsewhere.

## Verification — what to run

```
# in-process tests (fast, no Docker, no NATS required):
cd rs/mcp && cargo test --test pump --test jsonrpc --test stdio \
                       --test streaming --test tokens

# real-NATS smoke (opt-in; requires `just nats` or `just up`):
cd rs/mcp && cargo test --test nats_smoke
# or skip:
OPENSTORY_NATS_URL=skip cargo test --test nats_smoke

# testcontainers (requires Docker):
cd rs/mcp && cargo test --test testcontainer_nats

# all of it:
cd rs/mcp && cargo test                    # ~31 tests, ~3s with Docker present
```

Manual smoke against a running server:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"manual","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"subscribe_tokens","arguments":{"session_id":"<your-session-id>"}}}' \
  | OPENSTORY_NATS_URL=nats://localhost:4222 ./rs/mcp/target/release/open-story-mcp
```

Fail-loud verification:

```bash
OPENSTORY_NATS_URL=nats://127.0.0.1:9999 ./rs/mcp/target/release/open-story-mcp
# expect: nonzero exit, clear error message, no silent fallback
```

## File index (for the reviewer)

| Path | Role |
|---|---|
| `rs/mcp/src/subscription.rs` | `Subscribe` trait, `Subscription`, `StreamEvent`, `CancelGuard`, `pump_subscription` |
| `rs/mcp/src/nats_bus.rs` | Production `Subscribe` impl wrapping `open_story_bus::NatsBus` (~70 lines) |
| `rs/mcp/src/stdio.rs` | `run<S: Subscribe>` generic transport |
| `rs/mcp/src/bin/open-story-mcp.rs` | Fail-loud binary entry |
| `rs/mcp/tests/common/mod.rs` | `LoopbackSubscriber` (test fixture) + IngestBatch builders |
| `rs/mcp/tests/common/nats_container.rs` | Testcontainer helper (`nats:2.10` + JetStream) |
| `rs/mcp/tests/pump.rs` | Pure pump + CancelGuard unit tests |
| `rs/mcp/tests/testcontainer_nats.rs` | Real-bus, real-binary integration suite |
| `docs/research/streaming-mcp/` | This design garden (MOTIVATION/PLAN/TESTS/CONVERGENCE) |
