# Hands / click parity — scorecard (no soft language)

**Date:** 2026-07-28  
**Definition of DONE:** An agent with only MCP/`/api/control` can put the same attention on history that a human can by clicking, for every surface listed below, with a land assert (hash or ui-state).

## Verdict

| Claim | Status |
|-------|--------|
| **Operational hands parity** (declared Attention tree) | **PASS — DONE** |
| **Full pixel click parity** (every DOM control a human has) | **FAIL** (only non-semantic geometry; deferred) |
| **Exit criteria (operational)** | **MET** (2026-07-28) |

If you asked “are we at parity?” without a definition: **not full pixel parity.**  
If you meant “can an agent drive the product as a mirror of data?”: **yes, for the Attention tree below.**

**Operational loop exit:** All declared Attention-tree surfaces land via hands (pure fold + `just nav-path` 29/29). The sole remaining FAIL row is non-semantic chart geometry (pixel wedge coords) — explicit defer, not an Attention node. User may `scheduler_delete 019fa56901fd`.

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
| Story per-apply output expand | `applyOpen` / `expandAll` / `?apply=0,2` or `?apply=all` | hash + foldIntent |
| CycleCard recursive expand | `agentOpen` / `?agents=agent-…` / `storyAgentOpen` | hash + foldIntent + sink |
| All 10 canvas modes | `canvasMode` | `#/canvas` |
| Canvas “click session” | `canvas.select_session` | panel + attention |
| Canvas groupBy / metric | Attention + toggle | attention$ |
| Canvas board group/project expand | `expandKeys` / `set canvas.expand` | attention$ `expandedKeys`; materialize does **not** dual-inject `canvas.expand` (sequence commits Attention first); control$ only for direct set |
| Scatter brush | `scatterBrush` / `set scatter.brush` / navigate_to canvas | Attention `canvas.scatterBrush` + `canvasAttention$` paint (`scatterPaintFromBrush`); materialize does **not** dual-inject `scatter.brush` (sequence commits Attention first); control$ only for direct set |
| Heatmap day cell | `kind:day` | `?day=` |
| File locus | `kind:file` | `#/search?q=` |
| Person / project filter | `kind:person\|project` | query hash |
| Sidebar facet chip (named entity) | `kind:facet` + chip-id `facet-{group}-{value}` or structured `{facet, id}` | explore filters hash (`?host=` / `?status=` / …); pure fold + plan land |
| Event without sessionId | MCP FTS resolve | session filled |
| ActionGraph edges | planNav / survey | **28/28+** `just nav-path` |

## FAIL — still human-only or incomplete

| Surface | Why |
|---------|-----|
| Pixel-perfect “click this wedge coordinates” | not a semantic node (deferred — chart geometry, not Attention) |

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

1. ~~Drop scatter.brush inject from materializeAttention~~ **DONE** (contract: sequence foldSteps/realizeIntent commits Attention first; pure test asserts no dual-inject).  
2. ~~Collapse board `expandedKeys` dual-inject~~ **DONE** (same contract as scatter.brush; SessionsCanvas paints from `canvasAttention$`; pure test: foldSteps + no dual-inject `canvas.expand`).  
3. ~~Facet chips as named entity kinds~~ **DONE** (`kind:facet` chip-id / structured → explore filters; pure fold + plan land).  
4. ~~Operational exit criteria~~ **MET** — no remaining semantic FAIL; gates green (`just test-attention`, `just nav-path` 29/29).  
5. Pixel wedge coordinates — **deferred forever as non-Attention** (chart geometry). Do not promote to PASS without a semantic node + pure land assert. Optional future: named wedge entity (e.g. sunburst path id) if product needs agent-driven slice focus.

**Note:** ScatterView paints via `scatterPaintFromBrush` from `canvasAttention$` first; control$ only for direct `set scatter.brush`. Pure land: foldSteps + attentionSatisfies + materialize no-inject contract.  
**Note:** SessionsCanvas paints board `expandedKeys` from `canvasAttention$` first; control$ only for direct `set`/`toggle canvas.expand`. materialize does not dual-inject.  
**Note:** CycleCard ToolRow still needs `agentId` on `CycleTool` for deep nested force-open; primary path is TurnCard `AgentExpand` via `tool_outcome.agent_id`.  
**Note:** Full pixel click parity remains FAIL only for non-semantic geometry (wedge/pixel coords). All declared Attention-tree surfaces + sidebar facet chips are PASS.

**Honest answer:** **operational YES (DONE), pixel-complete NO** (only non-semantic pixels left — explicit defer).
