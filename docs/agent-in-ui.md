# Agent-in-UI seam

**Product path first:** [README — Using Open Story](../README.md#using-open-story). This page is the control/ui-state seam.

**Mission fit:** Open Story's mission is **read your agent history**. Agent-in-UI
is not a peer mission — it is the **attention layer** on top of that history.
Driving the UI is aligned when it **shows or navigates history** (focus an
event, open Story/Explore, present a finding, filter the fleet). Chrome for its
own sake fails mission fit. Doctrine: **the agent may steer the mirror; it may
not rewrite history.**

OpenStory's dashboard is a **sink**: it reacts to a stream of messages and
redraws. That makes it drivable and observable through two mirror-image halves —
**control** (what the dashboard shows) and **ui-state** (where attention is).
Both flow over the one WebSocket every dashboard is tuned to. Neither touches
the *observed sources* — this steers the mirror, never the watched.

> Sovereignty: control only changes what the dashboard *displays*. It never
> mutates a transcript, an agent, or a file. Commands land on `ui.*` only, never
> `events.*`. Every command is visible in the UI ("▸ driven by X"), dismissible,
> and itself auditable/replayable.

## WRITE — drive the dashboard (`ui_control` MCP tool)

POST a control *intent*; the server broadcasts it; every dashboard reacts.

**MCP:** `ui_control { action, params }` → returns `{ ok, delivered }`
(`delivered` = how many dashboards received it).
**HTTP:** `POST /api/control { action, params, issuer? }`.

The command vocabulary (`interpretControl` in the UI resolves each into a typed
`UIControlAction`). Full map: `docs/research/agent-ui-control-map.md`.

| action | params | effect |
|--------|--------|--------|
| `open_view` | `{ route }` **or** `{ view, sessionId?, detailView?, eventId?, filePath?, searchQuery?, userFilter?, timeFilter?, agent?, project?, … }` | navigate any bookmarkable hash state (legacy `/overview?…` aliases onto Explore) |
| `focus_event` | `{ sessionId, eventId, view?, spotlight?, clipAt? }` | open one event in Explore/Story; `spotlight:true` = full-screen presentation |
| `present` / `announce` / `highlight` | `{ message\|note, sessionIds?, route?, spotlight? }` | banner + session spotlight, optional jump; `spotlight:true` = title card |
| `query` / `filter` / `set_filter` | `{ project\|agent\|user\|status\|host\|branch\|day\|range\|search\|q\|sort }` | narrow Explore |
| `toggle` | `{ target, value }` | flip a registered view knob (`canvas.mode`, `story.sort`, `theme`, `session.lens`, `spotlight=off`, …) |
| `set` | `{ target, …fields }` | structured multi-field change (e.g. `scatter.brush`); `target:"draw.scene"` / `"draw.clear"` also take `scope` |
| `draw` | `{ strokes\|recipe, scope?, clear?, mode?, label?, visible?, target?, reelId?, beatIndex? }` | ink on the `ui.*` overlay (never history). `scope:"here"` (default) lands on the human's current view context — the glass they are looking at; `scope:"board"` targets the Draw tab's paper. An explicit `reelId` + `beatIndex` targets that reel slide and beats `scope` |
| **reel export** | — | **human affordance in v1:** reel HTML/snapshot export (preview + scan gate) belongs to the human; agents read reels via `list_reels` / `get_reel` MCP but export decisions are human-driven. |

Examples:

```jsonc
{ "action": "open_view", "params": { "route": "#/explore/SES/conversation?agent=grok" } }
{ "action": "open_view", "params": { "view": "explore", "sessionId": "SES", "detailView": "conversation" } }
{ "action": "focus_event", "params": { "sessionId": "SES", "eventId": "EVT", "spotlight": true } }
{ "action": "toggle",    "params": { "target": "canvas.mode", "value": "sunburst" } }
{ "action": "query",     "params": { "agent": "grok", "range": "7d" } }
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

**MCP:** `where_is_user {}` (no args) → an agent-friendly shape with HashRoute
parity fields when the client reported them:

```jsonc
{ "present": true, "view": "explore", "kind": "navigate",
  "session_id": "…", "event_id": "…", "detail_view": "conversation",
  "filters": { "agent": "grok" }, "file_path": null,
  "user_filter": null, "time_filter": null, "search_query": null,
  "spotlight": null, "present_message": null,
  "layout": {
    "targets": [{ "kind": "event", "id": "…", "rect": { "x": 0.2, "y": 0.3, "w": 0.5, "h": 0.12 } }],
    "focus": { "kind": "event", "id": "…", "rect": { "x": 0.2, "y": 0.3, "w": 0.5, "h": 0.12 } },
    "viewport": { "w": 1440, "h": 900 },
    "at": "…"
  },
  "at": "2026-07-02T12:42:27.507Z",
  "summary": "the user is on 'explore' viewing session … focused on event … layout focus event:…" }
```

`layout` is **layout eyes** — DOM bounding boxes in normalized viewport space
(0..1) for marked glass targets (`data-os-target` / `data-event-id`). Use
`layout.focus.rect` with `draw` (or recipe `layout-ring`) to ring what attention
is on. Absent when nothing measurable is on screen yet.

Ink lands on one of **three surfaces**, and each reports itself differently —
annotation is deictic, so ink lives with the thing it points at:

- **The board** (the Draw tab's global paper, `draw$`) → `pen`, **pen eyes**: a
  bounded snapshot with `stroke_count`, `kinds`, `bounds`, optional `label`, and
  a capped `strokes[]` list (paths downsampled). Debounced whenever board ink
  changes, from freehand on the Draw tab or an agent `draw` with `scope:"board"`.
  Empty board → `pen.empty: true`.
- **The glass** (whatever view the human is on — story, explore, live, the reels
  list) → `glassInk: { key, stroke_count }`, where `key` is the context identity
  (`"story:SES"`, `"live"`, …). Written by human freehand in annotate mode
  outside the Draw tab and by an agent `draw` with the default `scope:"here"`.
  The frame keeps the route's `session_id` / `detailView`, so the ink is always
  readable against what it points at.
- **A reel slide** → `beatInk: { reelId, beatIndex, stroke_count, empty, … }`,
  1:1 with the playing beat. Written by human Annotate inside the player and by
  an agent `draw` carrying `reelId` + `beatIndex`.

All three are `ui.*` only — never observed history. `where_is_user` projects
`pen` (plus `layout`, `annotate`, `reel_id`); `glassInk` and `beatInk` ride on
the raw frame, so read them from `GET /api/ui-state` → `ui_state`.

`present: false` means no interaction has been recorded yet (position unknown).

**HTTP:**
- **Poll:** `GET /api/ui-state` → `{ ui_state: { at, kind, view, session_id?, detailView?, eventId?, filters?, … }, tempo }`.
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
  as a proper CloudEvent (IngestBatch) to the authored `ui.*` JetStream stream
  (`ui_events::ui_subject`, strictly separate from the observed read-only
  `events.*`); the MCP consumes `ui.>` through the SAME typed pump as observed
  events (`bus::subscribe_typed` + `pump_subscription`) and shapes each with
  `ui_state_notification`. One bus, one source→sink graph, sovereignty intact.

## PACING — act in the user's rests

Don't drive over the user's shoulder. `GET /api/ui-state` also returns a `tempo`
block — the rhythm of their recent interactions — so an agent can act in the
**rests** (idle gaps) and hold during activity:

```jsonc
{ "ui_state": { … },
  "tempo": { "active_now": true,      // last interaction within 8s
             "rest_ms": 389,          // how long they've been idle
             "last_activity_ms": 1783027831091,
             "cadence_ms": 750 } }    // median gap between interactions (their beat)
```

**Rule of thumb:** poll `tempo`; drive/annotate only when `active_now` is
`false` (they've paused). `cadence_ms` is their beat — match it, don't outrun it.
The pure logic is `ui/src/lib/tempo-profile.ts` (client) and
`ui_events::tempo_profile` (server); both use an 8s idle threshold.

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
