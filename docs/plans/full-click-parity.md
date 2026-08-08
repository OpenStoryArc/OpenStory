# Full click parity — how we get there

**Goal:** An agent with only MCP hands can put attention on **any event** and
**any graph interaction a human can click**, with land assertions.

**Law:** Drive the mirror (`ui.*`), never rewrite history (`events.*`).

---

## Picture (architecture)

```
  data (sessions, events, files…)
           │
           ▼
  Intent (navigate_to)  ──foldIntent──►  Attention  ──materialize──►  pixels
           │                                  │
           │                         route · canvas · spotlight
           │                                  │
           └── planNav steps (fallback) ──────┘
                    POST /api/control
```

**Attention** (`ui/src/lib/attention.ts`) is the pure tree of what the mirror
shows. **attention$** (`ui/src/streams/attention.ts`) is the reactive store.
Views are sinks. Agents drive Attention — not the DOM.

**Three trees, one product bar:**

| Tree | Parity means |
|------|----------------|
| Data (ENTITY_EDGES) | every edge has a verb + planNav hop |
| Route (HashRoute) | every attention state is bookmarkable |
| Pixel (local UI) | every human click has set/toggle or route |

---

## Hands (what the agent uses)

| Tool | Role |
|------|------|
| **`navigate_to`** | High-level: `{ kind, id, sessionId?, … }` → plan + execute. Primary hand. |
| **`ui_control`** | Low-level verbs when you already know the step. |
| **`where_is_user`** | Confirm land. |
| **`openstory_help` need=show-human** | Curriculum. |

---

## Phases

### P0 — Done / baseline
- focus_event any event (explore + story)
- open_view all tabs + canvas.mode
- planNav + landPatterns + nav_path.mjs
- story.details ?details=1
- toolcall→file search

### P1 — navigate_to + canvas session click ✅
- `navigate_to` control action + MCP tool
- `canvas.select_session` (same as human openSessionPanel)
- `canvas.mode` + select session in one plan
- Docs in hands / agent-in-ui

### P2 — Charts that filter ✅
- heatmap day → `navigate_to { kind: "day", id: "YYYY-MM-DD" }` → query day
- tool-flow agent → `set canvas.flow.agent`
- scatter brush already settable

### P3 — Story depth (partial ✅)
- sentence depth via `details: true` / `story.details`
- per-apply output expand via `applyOpen` / `?apply=0,2|all` ✅
- remaining: cycle cards, `#/story/SES/turn/N`

### P4 — Resolve event → session ✅ (MCP)
- `navigate_to` auto-FTS resolves `sessionId` when kind=event|turn|sentence and omitted
- pure `resolveSessionFromHits` in UI algebra

### P5 — Conformance ✅ (script)
- `scripts/nav_path.mjs` — all ENTITY_EDGES + all canvas modes + day
- pure `allReachablePairs` for graph completeness tests

### Remaining (not blocking “hands complete”)
- Turn number deep-link; CycleCard nested expand
- Dedicated `/api/nav/resolve` (MCP search path is enough for now)
- CI workflow job on nav_path.mjs
- Board expandKeys ✅ on Attention.canvas (navigate_to expandKeys / set canvas.expand)

---

## navigate_to contract

```jsonc
{
  "action": "navigate_to",  // or MCP tool navigate_to
  "params": {
    "kind": "event" | "session" | "file" | "person" | "project"
          | "turn" | "sentence" | "subagent" | "canvas",
    "id": "…",                    // event id, session id, path, user, …
    "sessionId": "…",             // required when kind needs it
    "eventId": "…",
    "view": "story" | "explore",  // default story for events
    "details": true,              // sentence depth
    "canvasMode": "gantt",        // when opening via canvas
    "spotlight": false
  }
}
```

Returns `{ ok, steps: [{action, params, hash?}], landed, ui_state? }`.

---

## Definition of done (full parity)

1. Any event id → explore focus OR story focus OR spotlight (1 tool call).  
2. Any canvas mode → mode on screen (toggle).  
3. Any session id from any chart mode → detail panel or explore (same as click).  
4. planNav(A,B) non-null for all ENTITY_EDGES pairs with sufficient context.  
5. nav survey 100% land on CI.  
6. Agent never needs the OpenStory repo to learn the hands.
