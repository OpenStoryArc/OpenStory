# Loop prompt: Attention tree + agent hands (full click parity)

Copy everything under **PROMPT** into a new agent session. Re-run / continue until exit criteria pass.  
Related: `docs/plans/full-click-parity.md`, `ui/src/lib/attention.ts`, `ui/src/lib/nav-path.ts`.

---

## PROMPT

```
You are extending OpenStory so the dashboard is a pure Attention tree over
user-owned data, and agents drive that tree with powerful MCP hands
(navigate_to / ui_control) — not DOM hacks.

## Soul (non-negotiable)
- Observe, never interfere. Drive ui.* only; never mutate events.* / transcripts.
- Functional-first: pure fold of Intent → Attention; side effects only in
  materializeAttention / injectControl / navigate.
- BDD/TDD: failing test first. Prefer pure unit tests (vitest) over E2E.
- Do not lose functionality. Prefer lift local useState into Attention over
  deleting features.
- Dogfood: REST :3002 + MCP + scripts/nav_path.mjs. Prefer API over guessing.

## One-line objective
Any agent can navigate_to any event / canvas graph / day / file without the
repo docs, and land is assertable; Attention is the single denotation of
"what the mirror shows."

## Mental model (for you)
- Attention ≈ Redux store for UI *attention* (not for event history).
- HashRoute = bookmarkable spine of Attention.
- Canvas selection / spotlight / details = Attention fields.
- navigate_to = action that folds into Attention (like a Redux action).
- React components = pure-ish sinks that render Attention + local paint only.
- Event store / RxJS sessions = separate stream (data), not the attention tree.

## Confidence ladder (order; each is a closed loop)

### L1 — Pure algebra green
- [x] attention.test.ts + nav-path.test.ts all pass
- [x] foldIntent covers: event, session, canvas+mode+select, day, file, person, project, sentence/turn
- [x] attentionSatisfies matches foldIntent outputs (+ property table)
  - gate: `just test-attention`

### L2 — Live land survey
- [x] scripts/nav_path.mjs → 28/28
- [x] Fix any red edge before adding features
  - gate: `just nav-path` (needs :3002 + :5173)

### L3 — Hands complete for events + graphs
- [x] navigate_to event without sessionId still works (MCP FTS resolve)
- [x] navigate_to session + every canvasMode lands #/canvas + selection
- [x] navigate_to day → explore?day=
- [x] openstory_help + initialize.instructions teach navigate_to first
  - gate: dogfood curl POST /api/control navigate_to cases; MCP tools/list has navigate_to

### L4 — Lift more of the tree into Attention (no feature loss)
- [x] Canvas groupBy/metric on Attention.canvas (+ sinks)
- [ ] Story nested expand (eval/applies) — still local useState
- [x] Explore detail tabs already route-owned
  - gate: unit test fold + live navigate_to with groupBy/metric

### L5 — Reactive cleanliness
- [x] syncAttentionFromRoute on every hash change
- [x] materializeAttention + realizeIntent path for navigate_to
- [x] control.ts: wsMessages$() not wsMessages$ (regression fixed)
- [ ] injectControl dual path for canvas still exists (acceptable dual until sinks only read attention$)
  - gate: vitest + navigate_to after hard refresh

### L6 — Conformance
- [x] docs/reports/nav-path.md from survey
- [x] docs/plans/full-click-parity.md
- [x] justfile recipes: `test-attention`, `nav-path`
  - gate: survey green; docs match code

## Exit criteria (STOP when all true)
1. L1–L3 green.
2. nav_path.mjs exit 0.
3. Agent can, with only MCP (no repo): land on an event with details, land on
   gantt+session, land on a heatmap day — confirmed by where_is_user or hash.
4. Attention remains pure (no fetch inside foldIntent/foldControl).
5. No regression: `cd ui && npx vitest run tests/lib/attention.test.ts tests/lib/nav-path.test.ts tests/lib/ui-control.test.ts`

## Anti-goals
- Playwright clicking CSS selectors as the primary driver.
- LLM-generated "what the session meant" stored as Attention.
- Rewriting all of App.tsx for aesthetics alone.
- Mutating observed agent history.

## How to work each iteration
1. Pick the lowest unfinished ladder level.
2. Write a failing pure test (or extend nav_path pair).
3. Implement minimum pure fold / sink.
4. Run gates.
5. If green, mark checkbox in this prompt's checklist in a commit or report.
6. Continue until exit criteria; then STOP and report:
   - what Attention can express
   - what still lives only in useState
   - survey score
   - suggested next lift

## Key files
- ui/src/lib/attention.ts          — pure Attention algebra
- ui/src/streams/attention.ts      — reactive Attention store
- ui/src/lib/nav-path.ts           — pathfinder / planNavigateTo
- ui/src/lib/action-graph.ts       — ENTITY_EDGES
- ui/src/lib/ui-control.ts         — interpretControl + navigate_to
- ui/src/App.tsx                   — materialize shell
- ui/src/components/canvas/*       — sinks for canvas Attention
- rs/mcp/src/tools/control.rs      — navigate_to MCP + event resolve
- rs/mcp/agent-docs/hands.md       — agent curriculum
- scripts/nav_path.mjs             — live land survey
- docs/plans/full-click-parity.md  — roadmap

## Env
- API http://127.0.0.1:3002  UI http://127.0.0.1:5173
- UI tests: cd ui && npx vitest run …
- MCP tests: cd rs && cargo test -p open-story-mcp --lib
```

---

## How to run `/loop`

1. Ensure OpenStory is up (`:3002` + `:5173`).
2. Copy the fenced **PROMPT** block above.
3. Start a loop session with that prompt (e.g. `/loop` + paste, or paste into a fresh agent).
4. Agent works the ladder until **Exit criteria**; then stops and reports.

## Status snapshot (2026-07-27, loop run)

| Level | Status |
|-------|--------|
| L1 algebra | ✅ 66 pure tests (attention + nav-path + ui-control) |
| L2 survey | ✅ 28/28 `just nav-path` |
| L3 hands | ✅ navigate_to event/canvas/day dogfood delivered |
| L4 lift more | ✅ canvas groupBy+metric; Story nested expand still open |
| L5 reactive | ✅ Attention fold path; inject dual residual OK |
| L6 docs | ✅ just recipes + plans |

**Next loop (optional):** Story eval/applies expand on Attention; retire inject dual for canvas.

Regenerate survey: `just nav-path`  
Algebra only: `just test-attention`
