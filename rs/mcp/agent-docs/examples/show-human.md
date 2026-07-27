# Example: point the human’s dashboard (attention layer)

**Need:** show-human  
**Law:** steers the mirror only (`ui.*`). Never rewrites history.

```jsonc
// 1. Where are they? Idle enough to drive?
{ "tool": "where_is_user", "args": {} }
// Prefer driving when tempo.active_now is false (see ui-state tempo).

// 2. Open Story for a session
{ "tool": "ui_control", "args": {
    "action": "open_view",
    "params": { "view": "story", "sessionId": "SESSION_ID" }
}}

// 3. Spotlight one event
{ "tool": "ui_control", "args": {
    "action": "focus_event",
    "params": { "sessionId": "SESSION_ID", "eventId": "EVENT_ID", "spotlight": true }
}}

// 4. Or a title card
{ "tool": "ui_control", "args": {
    "action": "present",
    "params": { "message": "Peak moment — sentence detector", "spotlight": true }
}}

// 5. Canvas mode
{ "tool": "ui_control", "args": {
    "action": "open_view",
    "params": { "view": "canvas" }
}}
{ "tool": "ui_control", "args": {
    "action": "toggle",
    "params": { "target": "canvas.mode", "value": "gantt" }
}}

// 6. Confirm
{ "tool": "where_is_user", "args": {} }
```

**Views:** `live` \| `explore` \| `story` \| `canvas` \| `ask` \| `users` \| `admin`  
**Explore detailView:** `events` \| `conversation` \| `plans` \| `graph` \| `search`  
**canvas.mode:** sunburst, board, treemap, gantt, scatter, flow, tool-adjacency,
agent-project, durations, heatmap  

Full reference: `openstory://docs/agent-in-ui`.
