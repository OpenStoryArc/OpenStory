# Streaming MCP — test harness

BDD-shaped specs. Each `describe / it` corresponds to a real Rust test. Red first, green by implementing the minimum.

## Stage A — protocol + tools

### A.0 Protocol framing

```rust
describe("when an MCP client sends a malformed JSON-RPC request") {
    it("returns a -32700 Parse error response with id=null")
}
describe("when an MCP client sends a request with an unknown method") {
    it("returns a -32601 Method not found response preserving the id")
}
```

### A.1 Initialize handshake

```rust
describe("when an MCP client sends `initialize` with protocol version 2024-11-05") {
    it("responds with server_info name='open-story-mcp' and capabilities.tools = {}")
    it("does not require any prior message — initialize is the first call")
}
describe("when an MCP client sends `notifications/initialized` after initialize") {
    it("emits no response (notifications never get responses)")
    it("transitions the server to 'ready' state for tool calls")
}
```

### A.2 Tool registry — `tools/list`

```rust
describe("when an MCP client calls `tools/list` after initialize") {
    it("returns the full tool set including list_sessions, session_synopsis, project_pulse, search")
    it("each tool definition includes inputSchema as a valid JSON Schema")
    it("each tool definition includes a description with example usage")
}
```

### A.3 `list_sessions` with filters (the immediate pain fix)

```rust
describe("when `list_sessions` is called with no filters") {
    it("returns all sessions in the test fixture (currently 12)")
    it("returns trim row shape: {id, label, project_id, start, last_event, event_count}")
    it("does NOT return tool_calls=0 stub fields — either real values or absent")
}
describe("when `list_sessions(days=1)` is called against a fixture with sessions spanning 7 days") {
    it("returns ONLY sessions whose last_event >= now - 24h")
    it("does NOT return sessions older than that, even if first_event is recent")
}
describe("when `list_sessions(project='OpenStory')` is called") {
    it("returns only sessions where project_name='OpenStory'")
    it("matches case-insensitively on project name")
}
describe("when `list_sessions(days=1, project='OpenStory', limit=10)` is called") {
    it("returns the intersection of all filters, capped at 10 rows")
    it("orders rows by last_event DESC so newest activity is first")
}
describe("when `list_sessions(limit=10)` is called against a 10K-session fixture") {
    it("returns a response body ≤ 10KB when serialized as JSON")
}
```

### A.4 Shape-mismatch repair (today's bug)

```rust
describe("when `list_sessions` and `session_synopsis` are called for the same id") {
    it("project_name matches between the two responses")
    it("event_count matches between the two responses")
    it("if list_sessions returns first_prompt, it equals session_synopsis.label or is absent — never empty string")
}
```

### A.5 `tools/call` end-to-end

```rust
describe("when an MCP client calls tools/call with name='session_synopsis' and a valid id") {
    it("returns the synopsis result wrapped in {content:[{type:'text',text:<json>}]} per MCP spec")
    it("returns an isError=false flag")
}
describe("when an MCP client calls tools/call with name='session_synopsis' and a non-existent id") {
    it("returns isError=true with a Not Found error message")
    it("does NOT return a JSON-RPC error (-32xxx) — tool errors are different from protocol errors")
}
```

## Stage A — performance gates

### A.6 Latency budgets (cargo bench or criterion)

```rust
describe("median latency of session_synopsis against a 50K-event fixture") {
    it("is < 50ms p50")
    it("is < 200ms p99")
}
describe("list_sessions(days=1) on a 10K-session fixture") {
    it("is < 100ms p50")
    it("serialized response body is < 10KB at limit=10")
}
```

## Stage B — subscriptions + streaming

### B.0 Subscription lifecycle

```rust
describe("when a client calls subscribe_session(sid)") {
    it("returns immediately with {stream_id, status:'started'}")
    it("the stream_id is a valid uuid v4")
}
describe("when 10 events for sid are published to NATS after subscription") {
    it("the client receives 10 stream notifications in order")
    it("each notification carries seq=1..10 monotonic")
    it("each notification's data matches the event published")
}
describe("when the client sends notifications/cancelled with the subscription's request id") {
    it("the server stops emitting stream notifications within 100ms")
    it("the server sends a final {stream_id, status:'cancelled', total_emitted:N} message")
    it("the NATS subscription is torn down (no zombie task)")
}
describe("when the client disconnects mid-stream (stdio closes)") {
    it("the actor cleans up within 1s")
    it("no further work is scheduled on the runtime for that subscription")
}
```

### B.1 Predicate filtering

```rust
describe("when subscribe_session is called with predicate='subtype = message.assistant.tool_use'") {
    it("delivers only events matching that subtype")
    it("non-matching events do NOT count against the bounded channel capacity")
}
describe("when the predicate is malformed") {
    it("the tool call returns isError=true with parse error details")
    it("no subscription is opened")
}
```

### B.2 Backpressure

```rust
describe("when a slow client (50ms per recv) is subscribed and 1000 events arrive in burst") {
    it("no events are dropped silently")
    it("if the bounded channel overflows, the next delivered event carries overflow_count > 0")
    it("the actor does not block other subscribers' deliveries")
}
```

### B.3 Multi-subscriber

```rust
describe("when 10 concurrent subscribers exist for the same session") {
    it("each subscriber receives every event published to that session")
    it("no event is delivered twice to the same subscriber")
    it("cancellation of one subscriber does not affect the others")
}
```

### B.3a High-fanout: 100 sessions × 1 subscriber each

Simulates an agent-like consumer (me) listening to many sessions at once.

```rust
describe("when 100 subscribers each listen to a different session and 1 event/session is published") {
    it("all 100 events arrive at their respective subscribers within 200ms p99")
    it("memory usage after 10k events across all subscribers stays bounded (delta < 50MB)")
    it("no event is misrouted — subscriber for sid_N only sees events for sid_N")
}
```

### B.3b Fan-out: 1 session × 100 subscribers

```rust
describe("when 100 subscribers all listen to the same session and 1 event is published") {
    it("100 notifications are delivered within 100ms p99")
    it("each subscriber sees the event exactly once with the same payload")
}
```

### B.3c Fan-in: 1 subscriber × N sessions via predicate / wildcard

```rust
describe("when one subscriber calls subscribe_agent(agent='claude-code') with 100 active sessions") {
    it("events from any of those 100 sessions arrive in publication order")
    it("the subscriber can cancel cleanly, tearing down all underlying NATS subjects")
    it("when a 101st session is created mid-stream, its events arrive without a re-subscribe")
}
```

### B.3d The dogfood test — an agent watching itself

```rust
describe("when an MCP client running inside an OpenStory-observed agent subscribes to its own session") {
    it("the subscriber receives its own tool-use events within 100ms of being written to NATS")
    it("the subscriber's own subscription event is NOT echoed back to itself (no feedback loop)")
    it("disconnect-and-reconnect resumes from the last delivered seq, not from the beginning")
}
```

### B.4 Projection subscriptions

```rust
describe("when subscribe_patterns(session=X) is called and a pattern is detected") {
    it("the pattern arrives at the client within 200ms of detection")
    it("the wire envelope says event_kind='pattern' (not 'event')")
}
```

### B.5 Cross-agent view

```rust
describe("when subscribe_agent(agent='bobby') is called") {
    it("events from sessions with agent=bobby arrive at the client")
    it("events from sessions with agent=claude-code do NOT arrive")
}
```

## Stage B — performance gates

### B.6 End-to-end streaming latency

```rust
describe("median latency from NATS publish to MCP notification arrival") {
    it("is < 50ms p50 on a quiescent system")
    it("is < 150ms p99 under a 100-msg/sec event rate")
}
describe("startup cost of a subscription") {
    it("is < 20ms p50 from subscribe call to first event delivered (when events are pending)")
}
```

## Fixture strategy

- **Unit tests** (protocol layer) — pure JSON in/out, no transport, no store. Hand-built JSON values.
- **Integration tests** (tools dispatch) — in-memory transport pair + temp-dir SqliteStore seeded with fixture events.
- **Stage B subscription tests** — in-process NATS via `async-nats-server` test harness, or mock `Bus` impl that publishes synthetically. Prefer the latter for speed; the real-NATS test runs nightly.
- **Performance gates** — `criterion` benches against the SQLite fixture used in integration tests. CI runs them on a sized runner; local just measures and prints.
