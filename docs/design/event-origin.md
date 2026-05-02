# Design: Event Origin

**Status:** Draft
**Date:** 2026-04-19
**Related:** `design-two-streams.md`, `design-domain-events.md`, `design-turn-identity.md`

## Problem

OpenStory was built for a single person observing their own Claude Code sessions on their own machine. Today's `source` URIs are placeholders (`"arc://test"`), there's no notion of *where* an event came from, and sessions can't be grouped by host or user.

Recent work (the Figma Make prototype, the Bobby/OpenClaw agent on a Hetzner VPS) points in the same direction: OpenStory increasingly observes **multiple machines**, and users want to partition the view by who/where.

The prototype models this as a "Person" primitive: `Person → Session[] → Event[]`. This design argues that's the wrong primitive — and proposes one that emerges more cleanly from the data itself.

## What emerges from the data

Every CloudEvent OpenStory ingests has, unavoidably, a chain of origin that exists **without us inventing anything:**

```
Host → Agent → Project → Session → Turn → Event
```

- **Host** — where the watcher/hook runs. `gethostname()`. Can't not exist.
- **Agent** — the tool self-declares: `"claude-code"`, `"pi-mono"`, `"hermes"`. Already a CloudEvent extension.
- **Project** — the directory the session operates in. In the JSONL file path.
- **Session** — a UUID Claude Code itself generates.
- **Turn** — detected by the `eval_apply` patterns crate; the natural unit of agent action.
- **Event** — the atom.

"Person" is **not** in this chain. Person is a human category we lay on top — a label that groups one or more hosts. It's a UI convention, not a protocol primitive.

This design formalizes the origin chain and defers Person to a client-side concept.

## Design

### 1. Source URI scheme

Every CloudEvent carries a `source` URI with this shape:

```
arc://{host}/{agent}/{project}/{session}
```

Examples:

```
arc://max-macbook.local/claude-code/openstory/abc-123-def
arc://bobby-vps/claude-code/observer-prototype/def-456-ghi
arc://max-macbook.local/pi-mono/client-project/xyz-789-aaa
arc://max-macbook.local/hermes/sandbox/aaa-000-bbb
```

Scheme stays `arc://`. Host goes in URI authority. Agent / project / session are path segments. Standard URL parsing handles it — no custom regex.

### 2. CloudEvent extensions (denormalized indexes)

The source URI is canonical truth. For index-friendly querying, promote derivable parts to extensions:

```json
{
  "specversion": "1.0",
  "id": "...",
  "source": "arc://max-macbook.local/claude-code/openstory/abc-123",
  "type": "io.arc.event",
  "time": "2026-04-19T12:00:00Z",

  "host":       "max-macbook.local",          // NEW — indexed
  "agent":      "claude-code",                 // existing
  "project":    "openstory",                   // NEW (optional) — indexed
  "session_id": "abc-123",                     // existing
  "subtype":    "message.assistant.text"       // existing
}
```

These extensions are **always derivable from `source`.** They exist as denormalized columns for efficient filtering, nothing more.

### 3. NATS subject hierarchy

Current: `events.{project}.{session}.main`

Proposed:

```
events.{host}.{agent}.{project}.{session}.main
events.{host}.{agent}.{project}.{session}.agent.{subagent_id}
```

Wildcards give filtering at the bus, free of code:

```
events.max-macbook.local.>                      — one machine
events.*.claude-code.>                          — any machine, Claude Code only
events.*.*.openstory.>                          — openstory project, any machine
events.max-macbook.local.*.*.abc-123.>          — one specific session
```

### 4. Person, as a client-side concept

No `people` table. No `/api/people` endpoint. Person lives entirely in the browser:

```ts
type Person = {
  id: string;          // local UUID
  name: string;        // "Max", "Bobby"
  color: string;       // hex or derived from hash(id)
  hosts: string[];     // ["max-macbook.local", "work-laptop"]
};
```

Stored in `localStorage` (sovereignty — your machine, your labels). Max's filter "my work" might map to `[macbook-pro.local, work-laptop]`. Bobby's events can show as their own persona if the user chooses.

Querying becomes straightforward:

```ts
// REST — one query per host, or a single endpoint with repeated param
/api/sessions?host=max-macbook.local&host=work-laptop

// WebSocket
/ws?host=max-macbook.local&host=work-laptop
```

Both translate to NATS subscriptions on the corresponding subjects.

### 5. Where host comes from at ingest

| Entry point | How host is determined |
|---|---|
| Local file watcher | `hostname::get()` in Rust stdlib at process start |
| HTTP `/hooks` endpoint | `Config.host_override` (per-machine config), fallback to `gethostname()` |
| NATS fan-in (Bobby pattern) | Already set by the remote publisher before publishing |

This matches "observe, never interfere" — each machine declares itself, we don't guess.

## Migration

Existing events have `source: "arc://test"` and no host extension. On read:

- If source doesn't match the `arc://{host}/...` shape, synthesize `host = "legacy"`
- Legacy events appear under a "legacy" host in UI views
- No backfill required; nothing breaks
- New events get proper URIs after deploy

## Files to touch

| Layer | File | Change |
|---|---|---|
| Core types | `rs/core/src/origin.rs` (new) | `EventOrigin` struct + `to_source_uri()` / `from_source_uri()` |
| Translators | `rs/core/src/translate.rs`, `translate_pi.rs`, `translate_hermes.rs` | Build source URI + set `host` at translation time |
| Bus | `rs/bus/src/` | Subject construction from `EventOrigin` |
| Store (SQLite) | `rs/store/src/sqlite_store.rs` | Add `host`, `project` columns + indexes |
| Store (Mongo) | `rs/store/src/mongo_store.rs` | Same fields + indexes |
| EventStore trait | `rs/store/src/event_store.rs` | `list_sessions` accepts `host: Option<&str>` filter |
| Server API | `rs/server/src/api.rs` | `?host=` query param |
| WebSocket | `rs/server/src/ws.rs` | `?host=` filter in WS handshake |
| Config | `rs/server/src/config.rs` | `host_override` field |
| UI types | `ui/src/types/person.ts` (new) | `Person` + localStorage repo |
| UI filter | `ui/src/streams/connection.ts` | Append `?host=` to WS URL |

**Estimated new code:** ~300 Rust + ~150 TypeScript.

## Testing strategy

1. **Origin round-trip property test** — for any valid `EventOrigin`, `from_source_uri(to_source_uri(o)) == o`.
2. **EventStore conformance extension** — add host-filter cases to the existing 47-helper conformance suite; both SQLite and Mongo must return the same results for `list_sessions(host="max-macbook.local")`.
3. **NATS wildcard integration test** — publish to 3 subjects across 2 hosts; verify a consumer with `events.max-macbook.>` gets exactly the expected subset.
4. **Migration test** — feed a "legacy" event with `source="arc://test"` through ingest; assert it lands with `host="legacy"` and the rest of the pipeline treats it normally.

## What this buys on day one

- Events from laptop + VPS are distinguishable in the data.
- NATS subscribers filter at the bus — no per-client event fan-out on the server.
- DB queries by host are cheap (single indexed column).
- UI "Person" is pure client-side convention — your mapping, your machine, sovereign.

## Open questions

1. **Host naming normalization.** Should we lowercase hosts at ingest? Strip trailing `.local`? Or accept as-is and let the user deal with it?
2. **Agent identity vs. user identity.** "Bobby" is really a Claude Code session running under a unix user on a VPS. Is Bobby a *host* label or an *agent instance* label? (Current proposal: host. Feels right but deserves a beat.)
3. **Multi-tenancy later.** If OpenStory ever becomes a team product, the current design gives us `host` cleanly but no real user identity. A second migration would add `owner` as a separate extension, orthogonal to `host`.

## Prototype

A frontend experiment proving the hierarchy-browsing UX lives at
`openstory-ui-prototype/experiments/origin-chain/`. It uses real OpenStory session data (captured via `scripts/capture-origin-fixtures.py`), decorates it with synthesized origin URIs, and validates:

- Source URI parse / render round-trip
- NATS subject wildcard matching against origins
- Hierarchy tree narrows correctly as a subject filter is typed
- Mocked API (MSW) serves fixtures identically to the real server

See that project's `README.md` for details.
