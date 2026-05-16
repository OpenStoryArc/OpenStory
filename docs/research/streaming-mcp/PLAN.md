# Streaming MCP — plan

Two stages. All Rust. Tests first. Each stage is shippable; stage B is where the real prize lives.

## Stage A — Rust MCP, parity + filters

**Goal:** the existing Python MCP wrapper is replaced by a Rust binary that speaks JSON-RPC over stdio, calls `open-story-store::queries` directly, and supports the filter args (`days`, `project`, `limit`, `after`) that make `list_sessions` actually usable.

**Crate:** `rs/mcp/` (workspace member `open-story-mcp`).

**Layout:**
```
rs/mcp/
├── Cargo.toml
├── src/
│   ├── lib.rs           — re-exports
│   ├── protocol.rs      — JSON-RPC types + handler dispatch (pure, no I/O)
│   ├── transport.rs     — stdio frame reader/writer (tokio)
│   ├── server.rs        — wires protocol + transport + tool registry
│   ├── tools/
│   │   ├── mod.rs       — tool trait + registry
│   │   ├── sessions.rs  — list_sessions, session_synopsis, session_story
│   │   ├── search.rs    — search, agent_search
│   │   └── pulse.rs     — project_pulse, daily_token_usage
│   └── bin/
│       └── open-story-mcp.rs   — main entry
└── tests/
    ├── jsonrpc.rs       — protocol layer integration
    ├── tools.rs         — tool dispatch end-to-end
    └── helpers.rs       — in-memory transport + store fixture
```

**Protocol decision:** hand-rolled JSON-RPC 2.0 over stdio. Not `rmcp` — the MCP message set is small (initialize, tools/list, tools/call, notifications/initialized, notifications/cancelled), the streaming primitives we want aren't fully spec'd, and rolling our own gives stage B room to add custom streaming notifications without fighting an abstraction.

**Side effects at the edges (principle #4):**
- `protocol.rs` is pure: `handle_request(&ToolRegistry, JsonRpcRequest) -> JsonRpcResponse`.
- `transport.rs` owns the stdio task.
- `tools/*.rs` are async functions that take `&StoreState` (or an Arc to it) and return `serde_json::Value`. No I/O outside the store calls.

**Tools shipped in stage A** (parity + the new filter args):
- `list_sessions(days?, project?, limit?, after?)` → trim row shape `{id, label, project, start, last_event, event_count}` (≤ 8KB at limit=10)
- `session_synopsis(id)` → unchanged shape
- `session_story(id)` → unchanged shape
- `search(q, days?, project?, limit?)` → events matching FTS
- `project_pulse(days?)` → unchanged
- `token_usage(days?)`, `daily_token_usage(days?)` → unchanged

**What stage A *does not* do:** subscribe, stream, push notifications, NATS. That's stage B.

**Stage A gate:** all current Python MCP use cases work against the Rust binary; `list_sessions(days=1)` returns ≤ 10KB for typical workloads; the shape-mismatch bug from today is fixed (one source of truth: `queries::list_sessions` returns full rows).

## Stage B — Streaming subscriptions

**Goal:** the binary subscribes to NATS and streams events/patterns/projections back to the client via MCP notifications. Client cancellation is first-class.

**New tools:**
- `subscribe_session(session_id, predicate?)` → stream of events for that session
- `subscribe_patterns(session_id?, type?)` → stream of `PatternEvent`
- `subscribe_anomalies(baseline?)` → stream of events diverging from baseline (stage B.2)
- `subscribe_agent(agent, since?)` → cross-agent view

**Streaming protocol over stdio:**
- Client calls `tools/call` with a `subscribe_*` tool. Server returns immediately with `{ stream_id: "uuid", status: "started" }`.
- Server emits `notifications/openstory/stream` with body `{ stream_id, seq, event_kind, data }` for each event.
- Client cancels via `notifications/cancelled` with the original request id; server sends final `{ stream_id, status: "cancelled", total_emitted: N }` and tears down the NATS subscription.
- Backpressure: bounded tokio mpsc channel per subscription (capacity 256); slow clients see `overflow_count` field on next event.

**Wire to NATS:** `tools/subscribe.rs` opens a NATS subscription to `events.{project}.{session}.>` (or wider for cross-agent). Each delivered message is converted to the wire envelope and written to the transport.

**Predicate language (stage B.1):** small and explicit. `field op value`, e.g. `subtype = "message.assistant.tool_use"`, `data.tool = "Bash"`. AND-of-clauses, no general expressions. Predicate evaluated on the server side before emit — agents don't pay for filtered-out events.

**Stage B gate:** I can run `subscribe_session(my_session_id)` from the MCP client, see my own events arrive within 50ms of being written, cancel mid-stream, and reconnect with no resource leak. Verified by the perf test in `TESTS.md` (Stage B perf).

## Out of scope (for now)

- HTTP+SSE transport for remote MCP. The first iteration is stdio only. Remote becomes a future stage C once we know the local shape is right.
- In-process embedding of the MCP actor inside `open-story-server` (currently a separate binary that connects to NATS like its siblings). The in-process consolidation can wait until we know streaming works.
- Auth/bearer tokens on the MCP transport itself — stdio is implicitly trusted; remote will need this.
- Rate limiting per subscription — bounded channel is enough until we have a real workload that exposes a need.

## Naming & ergonomics

- Binary: `open-story-mcp`
- Build: `cd rs && cargo build -p open-story-mcp`
- Install via Claude Code: `claude mcp add open-story stdio /path/to/open-story-mcp`
- Logging: `eprintln!` for human messages (matches the rest of the server); structured emit goes to a separate log file when configured.
