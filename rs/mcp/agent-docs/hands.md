# Hands — how to use OpenStory MCP

You are a first-class user of OpenStory. You do **not** need the OpenStory git
repo. Everything you need is this MCP: tools, this resource, and related
resources listed at the end.

## Law

1. **Read-only on history.** These tools observe what agents wrote. They never
   mutate transcripts, sources, or agent behavior.
2. **Prefer tools over memory** when the human asks about past work, files,
   cost, or “what happened.”
3. **Cite.** Report `session_id`, file paths, event ids, timestamps when present.
4. **Do not invent events.** Empty results mean empty data (or soft holes — see
   `openstory://docs/physics`), not permission to fabricate.
5. **Sentences are projections** of tool acts (SVO coordinates), not your
   private monologue and not labeled “intent.”
6. **Dashboard control is separate.** `ui_control` authors `ui.*` only — it
   steers the mirror, never rewrites history.

## Motions (need → tools)

| Need | Tools (preferred order) |
|------|-------------------------|
| **orient** — what is / was this session? | `list_sessions` → `session_synopsis` or `session_story` → optional `session_sentences` |
| **what-touched** — files / tools / narrative coordinates | `file_impact` → `tool_journey` → `session_sentences` → `session_patterns` |
| **find** — across fleet history | `agent_search` or `search` → then `session_story` on hit sessions |
| **cost** | `token_usage` / `daily_token_usage` |
| **live** — watch as it unfolds | `subscribe_session` / `subscribe_tokens` |
| **show-human** — shared attention on the dashboard | **`navigate_to`** (primary) → `where_is_user`; low-level: `ui_control` |
| **stuck** | `openstory_help` with `need` or `topic` |

### navigate_to — primary click-parity hand

```jsonc
// Any event
{ "kind": "event", "id": "EVENT_ID", "sessionId": "SESSION_ID", "view": "story", "details": true }

// Any canvas graph + click a session (same as human bar/dot click)
{ "kind": "session", "id": "SESSION_ID", "canvasMode": "gantt" }
// or mode only:
{ "kind": "canvas", "id": "canvas", "canvasMode": "sunburst" }

// File / person / project / heatmap day
{ "kind": "file", "id": "sentence.rs" }
{ "kind": "person", "id": "max" }
{ "kind": "day", "id": "2026-07-27" }

// Event without sessionId — MCP resolves session via search automatically
{ "kind": "event", "id": "EVENT_ID", "details": true }

// Full Story card expand (details + eval-apply + event list)
{ "kind": "event", "id": "EVENT_ID", "sessionId": "SESSION_ID", "expandAll": true }
```

Prefer `navigate_to` over assembling multi-step `ui_control` by hand.

**Attention tree (UI architecture):** the dashboard is a pure tree of what is
shown (`Attention` = route + canvas selection + spotlight). `navigate_to` folds
an Intent into Attention, then materializes it. Agents express data; pixels follow.

Pure algebra: `foldIntent` / `realizeIntent` (attention), `planNav` (nav-path).
Survey: `node scripts/nav_path.mjs`.

## Default flows

### Pickup / resume a project

1. `list_sessions` — `{ "days": 1, "project": "PROJECT_NAME" }` (or omit project)
2. `session_story` — `{ "session_id": "SESSION_ID" }` — fact sheet (tools, sentences, prompts)
3. Drill only if needed: `session_transcript`, `tool_journey`, `session_errors`

### “What about this file?”

1. `recent_files` / `file_impact` on a known session, or `search` / `agent_search` with the path
2. `session_sentences` on sessions that hit it — verb/object coordinates
3. Optional: `ui_control` `open_view` with search or session so the human sees the same locus

### “Have we done this before?”

1. `agent_search` or `search` with the error / phrase
2. `session_story` or `session_errors` on the best hit

### Cost this week

1. `daily_token_usage` `{ "days": 7 }`
2. Optional `token_usage` scoped to `session_id`

### Live self-watch

1. Know your `session_id` (from the host / transcript path)
2. `subscribe_session` or `subscribe_tokens` with that id
3. Cancel via the host’s cancel / `notifications/cancelled` as your client supports

### Show the human (attention layer)

1. `where_is_user` — are they idle? (`GET` tempo: drive in rests when possible)
2. `ui_control` — e.g. open Story, focus an event, present a banner
3. Confirm with `where_is_user` again

Full verb map: `openstory://docs/agent-in-ui`.

**Click-parity pathfinder (UI):** shortest path over ActionGraph entity kinds →
control sequence → land on hash. In the OpenStory UI package:
`shortestEntityPath` / `planNav` / `landMatches` in `ui/src/lib/nav-path.ts`.
Live survey: `node scripts/nav_path.mjs` (event→turn, turn→sentence with
`?details=1`, toolcall→file search, …). Expand sentence depth:
`set { target: "story.details", open: true, sessionId, eventId }`.

## What not to do

- Dump the entire `session_transcript` first when `session_story` would do
- Treat sentence text as “the agent intended X”
- Drive the UI over an active user without checking tempo / rests
- Claim OpenStory rewrote history because you called a tool (it didn’t)

## Depth

| Resource | Contents |
|----------|----------|
| `openstory://docs/hands` | This file |
| `openstory://docs/physics` | Events, turns, outcomes, sentences, soft holes |
| `openstory://docs/agent-in-ui` | Drive/follow/replay the dashboard |
| `openstory://examples/pickup` | Worked pickup flow |
| `openstory://examples/file-locus` | Worked file history flow |
| `openstory://examples/show-human` | Worked attention flow |

Or call **`openstory_help`** with `need`: `orient` \| `what-touched` \| `find` \|
`cost` \| `live` \| `show-human`, or `topic`: a tool name / `physics` / `ui`.
