# Agent UI control map — driving OpenStory as a storytelling surface

*Sidequest on `feat/agent-ui-control`. Complements `docs/agent-in-ui.md` and
`docs/agent-ui-tours.md`.*

## Principle

**Drive the mirror, never the watched.** Control only steers what the dashboard
*shows* — hash routes, registered view knobs, present/spotlight overlays. It
never mutates transcripts, agent sources, or the observed `events.*` stream.

Full control here means **complete the map of bookmarkable UI state + registered
view knobs**, not inject browser RPA into the product.

## WRITE — verbs (`ui_control` → `POST /api/control`)

| Verb | Params | Resolves to |
|------|--------|-------------|
| `open_view` | `{ route }` **or** structured HashRoute fields | `navigate` |
| `focus_event` | `{ sessionId, eventId, view?, spotlight?, clipAt? }` | `navigate` or `spotlight` |
| `present` / `announce` / `highlight` | `{ message\|note, sessionIds?, route?, spotlight? }` | `present` or `title` |
| `query` / `filter` / `set_filter` | facets + `search`/`q` + `sort` + `range` | `navigate` → Explore |
| `toggle` | `{ target, value }` | component-local knob |
| `set` | `{ target, …fields }` | structured knob |

### open_view — any bookmarkable state

**Escape hatch (always works):** pass the full hash the human would bookmark.

```json
{ "action": "open_view", "params": { "route": "#/explore/SES/conversation?agent=grok&sort=recent" } }
```

**Structured params** (map 1:1 onto `HashRoute`):

| Param | HashRoute field | Example |
|-------|-----------------|---------|
| `view` | `view` | `live` \| `explore` \| `story` \| `canvas` \| `ask` \| `users` \| `admin` |
| `sessionId` | `sessionId` | session UUID |
| `detailView` | `detailView` | `events` \| `conversation` \| `plans` \| `graph` \| `search` |
| `eventId` | `eventId` | event UUID |
| `filePath` | `filePath` | `src/lib/ui-control.ts` |
| `searchQuery` | `searchQuery` | FTS query (with `detailView: search`) |
| `userFilter` | `userFilter` | Live tab person filter |
| `timeFilter` | `timeFilter` | `1h` \| `today` \| `week` \| `all` |
| `agent`, `project`, `user`, `status`, `host`, `branch`, `day`, `range`, `search`/`q`, `sort` | `explore.filters` / `explore.sort` | fleet facets on Explore |

### focus_event — one event, camera on

```json
{ "action": "focus_event", "params": { "sessionId": "…", "eventId": "…", "view": "story" } }
{ "action": "focus_event", "params": { "sessionId": "…", "eventId": "…", "spotlight": true, "clipAt": "## Where that leaves us" } }
```

### present — narration to the human

```json
{ "action": "present", "params": { "message": "Look at this failure cluster", "sessionIds": ["…"], "route": "#/story/…" } }
{ "action": "present", "params": { "message": "Have your agent read your agent history.", "spotlight": true } }
```

Dismiss spotlight/title: `toggle { "target": "spotlight", "value": "off" }` (or Esc / next navigate).

### Registered toggle/set targets

See `ui/src/lib/control-registry.ts` (`CONTROL_TARGETS`). High-value:

| Target | Values (closed where listed) |
|--------|------------------------------|
| `canvas.mode` | sunburst, board, treemap, gantt, scatter, flow, tool-adjacency, agent-project, durations, heatmap |
| `canvas.groupBy` / `canvas.metric` | dims / `events`\|`tokens` |
| `story.sort` | latest, active, tokens |
| `session.lens` | conversation, trace, subagents, details |
| `theme` | light, dark |
| `ask.question` | question id |
| `ribbon.compact` / `ribbon.collapsed` / `tokens.collapsed` | on \| off |
| `heatmap.dim` / `heatmap.weeks` | 2d\|3d / number |
| `spotlight` | off (dismiss) |
| `scatter.brush` | **set** — `{ev0,ev1,tok0,tok1}` |

Unknown targets no-op on the UI (open vocabulary); registry is for discovery + hints.

## READ — where is the user

`where_is_user` / `GET /api/ui-state` → summarized shape:

```jsonc
{
  "present": true,
  "view": "explore",
  "kind": "navigate",
  "session_id": "…",
  "event_id": "…",          // when reported
  "detail_view": "conversation",
  "file_path": "…",
  "filters": { "agent": "grok" },
  "user_filter": "katie",
  "time_filter": "today",
  "search_query": "auth",
  "spotlight": true,        // when client reports presentation state
  "present_message": "…",
  "at": "…",
  "summary": "the user is on 'explore' viewing session … / conversation focused on event …"
}
```

Source: latest interaction in the synthetic `openstory-ui` session (client
`interactionFromRoute` + view-local select/filter/zoom). Privacy: navigation
state only.

## Storytelling tour script (example)

```jsonc
// 1. Title card
{ "action": "present", "params": { "message": "Tonight's story: one Grok session.", "spotlight": true } }

// 2. Open Story for the session
{ "action": "open_view", "params": { "view": "story", "sessionId": "SES" } }

// 3. Spotlight the peak event
{ "action": "focus_event", "params": { "sessionId": "SES", "eventId": "EVT", "spotlight": true } }

// 4. Narrow fleet to this agent
{ "action": "query", "params": { "agent": "grok", "range": "7d" } }

// 5. Land on conversation in Explore
{ "action": "open_view", "params": { "view": "explore", "sessionId": "SES", "detailView": "conversation" } }

// 6. Confirm with where_is_user → detail_view should be "conversation"
```

curl form:

```bash
curl -s -X POST http://localhost:3002/api/control \
  -H 'content-type: application/json' \
  -d '{"action":"open_view","params":{"view":"explore","sessionId":"SES","detailView":"conversation"},"issuer":"story-tour"}'
```

## Gap table (after this pass)

| HashRoute / UI state | Driveable | Readable via where_is_user |
|----------------------|-----------|----------------------------|
| view | ✅ open_view / query | ✅ |
| sessionId | ✅ | ✅ session_id |
| detailView | ✅ structured + hash | ✅ when client posts (navigate) |
| eventId | ✅ open_view / focus_event | ✅ when client posts |
| filePath | ✅ structured + hash | ✅ when client posts |
| searchQuery | ✅ | ✅ when client posts |
| explore filters/sort | ✅ query + open_view facets + hash | ✅ filters |
| userFilter / timeFilter | ✅ structured + hash | ✅ when client posts |
| canvas.mode etc. | ✅ toggle/set | ⚠️ only if a view posts zoom/mode interactions |
| spotlight / title | ✅ focus_event / present | ✅ App posts spotlight/present_message on drive |

## Code map

| Layer | Path |
|-------|------|
| Pure interpret | `ui/src/lib/ui-control.ts` |
| Hash route | `ui/src/lib/hash-route.ts` |
| Control registry | `ui/src/lib/control-registry.ts` |
| control$ stream | `ui/src/streams/control.ts` |
| App sinks (nav/present/spotlight) | `ui/src/App.tsx` |
| Interaction report | `ui/src/lib/interaction.ts` |
| MCP tool | `rs/mcp/src/tools/control.rs` |
| HTTP control / ui-state | `rs/server/src/api.rs` |
| Agent-facing doc resource | `docs/agent-in-ui.md` → `openstory://docs/agent-in-ui` |
