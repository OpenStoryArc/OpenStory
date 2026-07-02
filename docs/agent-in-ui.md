# Agent-in-UI seam

OpenStory's dashboard is a **sink**: it reacts to a stream of messages and
redraws. That makes it drivable and observable by an agent through two mirror-
image halves — **control** (drive what the dashboard shows) and **ui-state**
(read where the user is). Both flow over the one WebSocket every dashboard is
tuned to. Neither touches the *observed sources* — this steers the mirror, never
the watched.

> Sovereignty: control only changes what the dashboard *displays*. It never
> mutates a transcript, an agent, or a file. Every command is visible in the UI
> ("▸ driven by X"), dismissible, and itself an event (auditable/replayable).

## WRITE — drive the dashboard (`ui_control` MCP tool)

POST a control *intent*; the server broadcasts it; every dashboard reacts.

**MCP:** `ui_control { action, params }` → returns `{ ok, delivered }`
(`delivered` = how many dashboards received it).
**HTTP:** `POST /api/control { action, params, issuer? }`.

The command vocabulary is four shapes (`interpretControl` in the UI resolves
each into a typed `UIControlAction`):

| action | params | effect |
|--------|--------|--------|
| `open_view` | `{ route }` or `{ view, sessionId? }` | navigate (route = `/canvas`, `/story/<id>`, `/overview?project=…`) |
| `present` / `announce` / `highlight` | `{ message, sessionIds?, route? }` | show a banner + spotlight sessions, with an optional jump |
| `toggle` | `{ target, value }` | flip a component-local control (`canvas.mode`, `heatmap.dim`, `story.sort`, `lab.open`, …) |
| `set` | `{ target, params }` | structured multi-field change (a brush box, a camera pose, a drill path) |

Examples:

```jsonc
{ "action": "open_view", "params": { "route": "/canvas" } }
{ "action": "toggle",    "params": { "target": "canvas.mode", "value": "delegation" } }
{ "action": "present",   "params": { "message": "Look here →",
                                     "sessionIds": ["0375729d-…"],
                                     "route": "/story/0375729d-…" } }
```

curl:

```bash
curl -s -X POST http://localhost:3002/api/control \
  -H 'content-type: application/json' \
  -d '{"action":"open_view","params":{"route":"/canvas"}}'
# → {"ok":true,"action":"open_view","delivered":2}
```

## READ — follow the user

When the user navigates, the UI emits a typed `Interaction`
(`navigate | filter | select | zoom | view`) to `POST /api/interactions`; the
server stores it as a CloudEvent in the synthetic `openstory-ui` viewing session
and exposes the latest.

**MCP:** `where_is_user {}` (no args) → an agent-friendly shape:

```jsonc
{ "present": true, "view": "overview", "kind": "navigate",
  "session_id": null, "at": "2026-07-02T12:42:27.507Z",
  "summary": "the user is on 'overview'" }
```

`present: false` means no interaction has been recorded yet (position unknown).

**HTTP:**
- **Poll:** `GET /api/ui-state` → `{ ui_state: { at, kind, view, session_id? } }`.
- **Follow (live, MCP):** `subscribe_ui_state` (no args) — a streaming tool that
  emits a `notifications/openstory/ui_state` frame each time the user moves:

  ```jsonc
  { "jsonrpc": "2.0", "method": "notifications/openstory/ui_state",
    "params": { "stream_id": "…", "seq": 3,
                "ui_state": { "present": true, "view": "canvas",
                              "session_id": null, "summary": "the user is on 'canvas'" } } }
  ```

  Pair it with `ui_control` to **follow → act**: read where the user landed,
  then drive from there.

  Under the hood this is fully on the bus: the server publishes each interaction
  to the authored `ui.*` JetStream stream (`ui_events::ui_subject`, strictly
  separate from the observed read-only `events.*`); the MCP consumes `ui.>` as
  raw frames (`bus::subscribe_raw`) and shapes each with `ui_state_notification`.
  One bus, one source→sink graph, sovereignty partition intact.

## The symmetry

A **command** says "make the UI show X"; an **interaction** says "the UI *is*
showing X because the user did it." Same typed vocabulary, opposite directions,
same pipe — so a user's interaction stream can be replayed *as* commands (redo a
journey), and an agent can drive *from* where the user is (`where_is_user` →
`ui_control`).

## Wiring the MCP

The `open-story-mcp` binary reads `OPENSTORY_API_URL` (default
`http://localhost:3002`) for both the read store and the control POST target.
Build from `rs/mcp` → `~/.local/bin`, then register via `/plugin` (see the repo's
MCP install notes). `ui_control` requires the API base to be set (it is, by
default).
