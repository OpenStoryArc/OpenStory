# Hands / click parity — scorecard (no soft language)

**Date:** 2026-07-28  
**Definition of DONE:** An agent with only MCP/`/api/control` can put the same attention on history that a human can by clicking, for every surface listed below, with a land assert (hash or ui-state).

## Verdict

| Claim | Status |
|-------|--------|
| **Operational hands parity** (declared Attention tree) | **PASS** |
| **Full pixel click parity** (every DOM control a human has) | **FAIL** |

If you asked “are we at parity?” without a definition: **not full pixel parity.**  
If you meant “can an agent drive the product as a mirror of data?”: **yes, for the Attention tree below.**

---

## PASS — agent can drive (land-asserted)

| Surface | How | Land |
|---------|-----|------|
| Any top-level view | `open_view` / navigate_to | hash `#/…` |
| Any event (Explore) | `navigate_to {kind:event}` / focus_event | `#/…/event/ID` |
| Any event (Story) | same, `view:story` | `#/story/…/event/ID` |
| Story ▾ details | `details:true` / `?details=1` | hash |
| Story eval-apply | `evalOpen` / `expandAll` / `?eval=1` | hash |
| Story event list | `eventsOpen` / `expandAll` / `?events=1` | hash |
| All 10 canvas modes | `canvasMode` | `#/canvas` |
| Canvas “click session” | `canvas.select_session` | panel + attention |
| Canvas groupBy / metric | Attention + toggle | attention$ |
| Heatmap day cell | `kind:day` | `?day=` |
| File locus | `kind:file` | `#/search?q=` |
| Person / project filter | `kind:person\|project` | query hash |
| Event without sessionId | MCP FTS resolve | session filled |
| ActionGraph edges | planNav / survey | **28/28+** `just nav-path` |

## FAIL — still human-only or incomplete

| Surface | Why |
|---------|-----|
| Individual apply-row expand inside eval-apply | local `useState` per apply |
| CycleCard recursive expand | local fetch + useState |
| Canvas board node expand (group/project drill path) | local `expanded` Set |
| Scatter brush (partial) | `set scatter.brush` exists; not in navigate_to table |
| Every sidebar facet chip as named entity | covered via query, not chip-id |
| Pixel-perfect “click this wedge coordinates” | not a semantic node |

---

## Architecture (what “tree” means)

```
Intent (navigate_to)
  → foldIntent → Attention   // pure
  → materializeAttention     // ports only
  → React sinks paint
```

Redux analogy: **Attention = store for “what you’re looking at”**, not for event history.  
History is a separate stream (facts). Hands drive Attention only.

---

## Gates (must stay green)

```bash
just test-attention          # pure algebra
just nav-path                # live land (:3002 + :5173)
```

If either fails, **parity is FAIL** until fixed. No “mostly.”

---

## Next work that moves FAIL → PASS on remaining rows

1. Per-apply expand → Attention or hash `apply=N`  
2. Board expand keys → Attention.canvas.expandedKeys  
3. Collapse inject dual: canvas sinks read **only** attention$  

Until then, the honest answer remains: **operational YES, pixel-complete NO.**
