# MCP Server Architecture

*How `open-story-mcp` works — and why you can trust it.*

**Start here if you only need install + wire-up:** [README — Using Open Story](../README.md#using-open-story). This page is the trust model, tool surface, and source citations.

## What it is

`open-story-mcp` is a small, single-binary [Model Context Protocol](https://modelcontextprotocol.io/)
server that exposes OpenStory history over MCP — the same store the dashboard and
REST API use. It is how a session **reads itself across the whens**: *is writing*
(`subscribe_session`, `subscribe_tokens` — watch this session as it unfolds),
*has written* (story, transcript, sentences, patterns mid-session), *wrote*
(search, list, cost, “what did I do yesterday?”). One surface among several for
the same mission (**read your agent history**); all from your own store; all
read-only on history.

**Attention layer (not a second mission):** `ui_control`, `where_is_user`, and
`subscribe_ui_state` steer the dashboard to show or navigate that history —
shared attention, tours, focus. They author `ui.*` only, never `events.*`. Aligned
when pointing at history; not co-equal with reading it. Full seam:
[`docs/agent-in-ui.md`](agent-in-ui.md).

The crate lives at `rs/mcp/` and ships the `open-story-mcp` binary
(`rs/mcp/Cargo.toml:8`). It is a first-class member of the Rust workspace, built
and tested alongside the server.

This document grounds every architectural claim in the source. If a sentence
makes a claim about behavior, it cites the file and line that proves it.

## The one thing to understand first: it only reads, over REST

The MCP server **does not open your database.** Its query tools read OpenStory
through the same public REST API the dashboard uses (`http://localhost:3002` by
default). It holds no SQLite handle, no Mongo connection, no file descriptor on
your data directory.

This is the load-bearing design decision, and it is enforced by the type that
backs every query tool: `HttpEventStore` (`rs/mcp/src/http_store.rs:52`). It
implements OpenStory's `EventStore` trait
(`rs/mcp/src/http_store.rs:182`) — but instead of reading a database, each trait
method issues an HTTP `GET` and deserializes the response
(`rs/mcp/src/http_store.rs:77-98`). The endpoint map is documented inline and
verified against the server's routes (`rs/mcp/src/http_store.rs:18-35`; routes
confirmed in `rs/server/src/router.rs:96-241`).

Because it is read-only by construction, the *write* half of the `EventStore`
trait is implemented as hard errors, not silent no-ops
(`rs/mcp/src/http_store.rs:357-382`):

```rust
async fn insert_event(&self, _session_id: &str, _event: &Value) -> Result<bool> {
    Err(anyhow!("HttpEventStore is read-only: insert_event unsupported"))
}
```

There is no code path through which the MCP can write to your store, mutate an
event, or change agent behavior. That is not a policy — it is the absence of an
implementation.

## Why REST and not direct SQLite

An earlier version of the MCP opened the SQLite store directly. That coupled it
to a data directory resolved relative to the process's working directory, so
launching the MCP from `~` would open an empty `./data/open-story.db` and report
an empty index. Reading through the API removes that footgun and buys four
properties, called out in the source (`rs/mcp/src/http_store.rs:1-17`,
`rs/mcp/src/bin/open-story-mcp.rs:1-21`):

- **Sovereignty of launch / cwd-independence.** The binary needs only a URL, not
  a data directory. Launch it from anywhere; it always reads the same store.
- **Portability.** No database driver, no schema version coupling, no lock
  contention with the running server. One HTTP client.
- **Remote-capable.** Point `OPENSTORY_API_URL` at any OpenStory instance you
  control. The MCP and the store no longer have to live on the same machine.
- **Single-owner.** Reads flow through one front door — the API you already run
  and already secure. There is exactly one process with a database handle (the
  server), so there is one place to reason about access.

The one tool that doesn't read events — `session_plans` — reads through the same
seam via its own `PlanSource` abstraction (`rs/mcp/src/plan_source.rs:22`), with
an `HttpPlanSource` that calls `GET /api/sessions/{id}/plans`
(`rs/mcp/src/plan_source.rs:57`). Same principle, different endpoint.

## Transport: JSON-RPC 2.0 over stdio

The MCP speaks newline-delimited JSON-RPC 2.0 over stdin/stdout. The agent
launches the binary as a subprocess and exchanges one JSON object per line.

- **Framing.** The transport reads line-delimited messages, dispatches each, and
  writes one response line back; blank lines are skipped
  (`rs/mcp/src/stdio.rs:56-62`). A single writer task owns stdout so concurrent
  streams never interleave bytes on the wire (`rs/mcp/src/stdio.rs:36-48`).
- **Methods.** `initialize`, `tools/list`, and `tools/call` are handled.
  `initialize` and `tools/list` are pure functions with no I/O
  (`rs/mcp/src/protocol.rs:98-110`); `tools/call` is routed in the transport
  because it needs async access to the store and the writer channel
  (`rs/mcp/src/stdio.rs:110-137`).
- **Server identity.** `initialize` reports `serverInfo.name = "open-story-mcp"`
  and the crate version, echoing the client's protocol version (default
  `2024-11-05`) (`rs/mcp/src/protocol.rs:75-131`).
- **Robustness.** The protocol handler is fuzzed against malformed and hostile
  frames (truncated JSON, deep-nesting bombs, 1 MB method names, control
  characters) and must never panic — only return a Parse error or
  Method-not-found (`rs/mcp/src/protocol.rs:142-239`). The stdio boundary is a
  local trust boundary, but the parser is hardened anyway.

### Data flow

```
┌──────────────┐   stdio JSON-RPC    ┌────────────────┐
│ Coding agent │◀───────────────────▶│ open-story-mcp │
│ (MCP client) │   tools/call …      │   (this crate) │
└──────────────┘                     └───────┬────────┘
                                             │
                   query tools               │  subscribe_* tools
              (HttpEventStore /              │  (NatsBus)
               HttpPlanSource)               │
                     │                       │
            HTTP GET │                       │ NATS subscribe
                     ▼                       ▼
        ┌──────────────────────┐    ┌──────────────────┐
        │  OpenStory REST API  │    │  NATS JetStream  │
        │  (open-story-server) │    │  events.{…}.>    │
        └──────────┬───────────┘    └────────┬─────────┘
                   │                          │
                   ▼                          │ live CloudEvents
        ┌──────────────────────┐              │ as they happen
        │  EventStore (SQLite  │◀─────────────┘
        │  or Mongo) on disk   │   (server persists; MCP only reads)
        └──────────────────────┘
```

Two distinct paths, two distinct dependencies:

- **Query tools** never touch NATS or the disk — they GET the REST API.
- **Streaming tools** (`subscribe_session`, `subscribe_tokens`) subscribe to
  NATS for live events and never touch the REST API.

## The tool catalog

The tool surface is a static table (`rs/mcp/src/tools/mod.rs`); the
dispatcher matches `tools/call` names against it. There are **26 tools** —
including `navigate_to` (click-parity hand: any event / canvas graph),
`openstory_help` (in-band curriculum), the agent-in-UI seam (`ui_control`,
`where_is_user`, `subscribe_ui_state`), history/analytics query tools, and
streaming `subscribe_session` / `subscribe_tokens`.

**Agent-facing body schema (no git repo required):** `initialize.instructions`
lists motions; `resources/list` + `resources/read` serve embedded docs
(`openstory://docs/hands`, `physics`, `agent-in-ui`, examples under
`rs/mcp/agent-docs/`); `openstory_help` routes need/topic → the same curriculum.

### Curriculum + attention (no history mutation)

| Tool | What it returns |
|------|-----------------|
| `openstory_help` | In-band body schema: `{ need?, topic? }` → motion/tool cards + resource URIs. Pure; no HTTP. |
| `navigate_to` | Click-parity: `{ kind, id, sessionId?, canvasMode?, details? }` → dashboard plans multi-step drive (any event, any canvas mode + session select). |

### Query tools (REST, via `HttpEventStore` / `HttpPlanSource`)

| Tool | What it returns |
|------|-----------------|
| `list_sessions` | Sessions with optional `days` / `project` / `limit` / `after` filters; trim shape. |
| `session_synopsis` | Structured overview of one session: counts, time range, top tools. |
| `project_pulse` | Per-project activity over a window (`days`, default 7). |
| `session_activity` | Rich single-shot summary: first prompt, files touched, tool breakdown, errors, duration. |
| `session_story` | Fact-sheet: record types, tool histogram, pattern counts, turn-phase mix, sample sentences, opening/closing prompts. Native port of `scripts/sessionstory.py`. |
| `tool_journey` | Tool-call sequence in timestamp order: `{tool, file, timestamp}`. |
| `file_impact` | Files read/written with per-file counts, sorted by total ops. |
| `session_errors` | `system.error` events: `{timestamp, message}`. |
| `session_plans` | `/plan` documents written during a session (via `HttpPlanSource`). |
| `session_patterns` | Detected patterns; optional `pattern_type` filter. |
| `session_transcript` | Reconstructed message transcript; `assistant_only`, `limit`. |
| `session_sentences` | Narrative sentences (verb/object/human_prompt) from `turn.sentence` patterns. |
| `search` | Full-text (FTS5) search across indexed events. |
| `agent_search` | FTS results grouped by session, ranked — agent-friendly. |
| `project_context` | Recent sessions for a project. |
| `recent_files` | Files touched in a project's most recent sessions. |
| `token_usage` | Aggregated token usage + cost estimate (incl. cache fields). |
| `daily_token_usage` | Per-day token usage over a window. |
| `productivity` | Hourly activity density (events per hour-of-day). |

These all route through `dispatch_query_tool` (`rs/mcp/src/tools/mod.rs:218`),
which wraps each result in the MCP `{isError, content}` shape
(`rs/mcp/src/tools/mod.rs:257-268`). A request that can't reach the API degrades
per-call to an empty or `isError` result rather than killing the process
(`rs/mcp/src/http_store.rs:104-116`, `rs/mcp/src/bin/open-story-mcp.rs:18-21`).

### Streaming tools (live, via NATS)

| Tool | What it does |
|------|--------------|
| `subscribe_session` | Subscribe to a session's CloudEvents as they happen. Returns `{stream_id, status: "started"}` immediately, then emits `notifications/openstory/stream` lines per event. |
| `subscribe_tokens` | Subscribe to a session and stream a running token tally (input / output / cache_read / cache_create) per assistant message — an agent watching its own context consumption. Emits `notifications/openstory/tokens`. |

Streaming tools are special-cased in the transport because they need the
JSON-RPC id and the writer channel (`rs/mcp/src/stdio.rs:123-135`,
`rs/mcp/src/tools/mod.rs:165-184`). Each spawns a background task that pumps bus
events to stdout as notification lines tagged with a `stream_id`
(`rs/mcp/src/stdio.rs:198-223`, `:279-314`). The client cancels a stream by
sending `notifications/cancelled` referencing the original request id, which
tears down the matching task (`rs/mcp/src/stdio.rs:93-105`).

## Configuration

The binary reads exactly three environment variables
(`rs/mcp/src/bin/open-story-mcp.rs:33-37`):

| Variable | Default | Used by |
|----------|---------|---------|
| `OPENSTORY_API_URL` | `http://localhost:3002` | Every query tool — the REST origin it reads from. |
| `OPENSTORY_API_TOKEN` | *(none)* | Optional bearer token, sent as `Authorization: Bearer …` when the server has `api_token` set. |
| `OPENSTORY_NATS_URL` | `nats://localhost:4222` | The two `subscribe_*` streaming tools only. |

Auth is opt-in and matches the server: if you secure your OpenStory instance
with `api_token`, set `OPENSTORY_API_TOKEN` to the same value and every query
carries the bearer header (`rs/mcp/src/http_store.rs:86-88`). An empty token is
treated as "no auth" (`rs/mcp/src/bin/open-story-mcp.rs:35`).

**Fail-loud on NATS.** The binary refuses to start if NATS is unreachable
(`rs/mcp/src/bin/open-story-mcp.rs:39-44`). This is deliberate: a subscription
over a bus with no publisher *looks* like it works and silently delivers
nothing, which is worse than erroring out. If you only need the query tools and
have no NATS, that is the one rough edge to know about today.

## Trust and security properties

The MCP is built to the same soul as the rest of OpenStory — *observe, never
interfere* — and the code makes that auditable:

- **Read-only by construction.** History query tools go through `HttpEventStore`,
  whose write methods are unimplemented errors
  (`rs/mcp/src/http_store.rs:357-382`). The streaming tools only `subscribe`
  (`rs/mcp/src/stdio.rs:170`, `:250`). There is no write path.
- **No agent-behavior mutation.** The MCP is a subprocess the agent queries; it
  never sits in the agent's execution path and never injects, blocks, or
  rewrites anything. It translates your stored history into answers.
- **Your data stays in your instance.** The MCP holds no data of its own. It
  reads whatever OpenStory instance `OPENSTORY_API_URL` points at — by default,
  the one running on your own machine.
- **Remote is a choice you control.** Pointing the MCP at a remote instance is a
  single, explicit env-var change. Nothing reaches the network unless you set
  that URL; nothing authenticates unless you set that token.
- **Local trust boundary, hardened anyway.** stdio is a local boundary, but the
  protocol parser is fuzzed against hostile input and provably never panics
  (`rs/mcp/src/protocol.rs:191-239`).

## Verify it yourself

You don't have to take this document's word for any of it. Drive a raw stdio
handshake against the binary. With OpenStory running, the following sends
`initialize`, the initialized notification, and a `token_usage` call, then reads
the responses straight off stdout:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"manual","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"token_usage","arguments":{"days":7,"model":"sonnet"}}}' \
  | OPENSTORY_API_URL=http://localhost:3002 OPENSTORY_NATS_URL=nats://localhost:4222 open-story-mcp
```

You'll see the `initialize` result (with `serverInfo.name = "open-story-mcp"`),
the full 21-tool list, and a `token_usage` result with a `cost` field. To watch
the network side, point a proxy or `tcpdump` at port 3002 — every query tool is
a plain `GET` you can read.

For an end-to-end smoke that asserts the handshake and a `token_usage` result,
run the crate's tests: `cargo test -p open-story-mcp`.

## Further reading

- **Install + wiring** — `README.md` (Homebrew `openstory-mcp` formula, the two
  ways to wire it into an agent).
- **Streaming substrate design notes** — `docs/research/streaming-mcp/`
  (motivation, plan, test specs for the live subscription path).
- **The REST API the MCP reads** — `rs/server/src/api.rs`,
  `rs/server/src/router.rs`.
- **The `EventStore` contract** — `rs/store/src/event_store.rs`.
