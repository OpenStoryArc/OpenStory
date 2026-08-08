# Loop prompt: Attention tree + agent hands (full click parity)

Copy everything under **PROMPT** into a new agent session, or keep the scheduled
job `019fa56901fd` (5m) updated via `scheduler_create` with the same `task_id`.

Related: `docs/PARITY-HANDS.md`, `docs/plans/full-click-parity.md`,
`ui/src/lib/attention.ts`, `ui/src/lib/nav-path.ts`.

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
- Do not lose functionality. Prefer lift local useState into Attention over
  deleting features.
- Dogfood: REST :3002 + MCP + `just nav-path` / `just test-attention`.
- Scorecard truth only: docs/PARITY-HANDS.md PASS/FAIL — no soft "mostly."

## STRICT TDD / BDD (non-negotiable — no exceptions this loop)

Every firing that claims progress MUST follow red → green → refactor.

### Required order for each FAIL→PASS claim
1. **Name the FAIL row** from docs/PARITY-HANDS.md (or add a new FAIL row first).
2. **Write a failing pure test FIRST** (vitest). Prefer BDD shape:
   - describe("when X") / it("should Y")
   - or scenario(given, when, then) from ui/tests/bdd.ts
3. **Run the test and prove RED** — paste the failing test name + assertion
   failure in your report. If you cannot show red, you may not implement.
4. **Implement the minimum** to go green (pure fold first; sink second).
5. **Run the same test and prove GREEN**.
6. **Only then** update PARITY-HANDS.md FAIL→PASS (and only for land-asserted
   surfaces: pure fold + hash/attentionSatisfies, or nav_path land).

### Forbidden
- Implementing production behavior before a failing test exists for this firing.
- Claiming PASS without naming the failing test that went red then green.
- "Tests exist somewhere" — must be the test YOU wrote/extended THIS firing.
- Skipping red by writing the test after the code (if you slip: delete the
  production change, re-establish red, then re-implement).

### Report format (required every firing)
```
FAIL row: <exact row from PARITY-HANDS>
RED test: <file>::<describe/it name> — <one-line failure>
GREEN: <same test> — pass
Survey: <N/N or skipped: reason>
Remaining FAIL: <short list>
Exit criteria: met|not met
```

If the report lacks RED test name + evidence of red-then-green, the firing
does not count as progress.

## One-line objective
Any agent can navigate_to any event / canvas graph / day / file without the
repo docs, and land is assertable; Attention is the single denotation of
"what the mirror shows."

## Mental model
- Attention ≈ Redux store for UI *attention* (not for event history).
- HashRoute = bookmarkable spine of Attention.
- navigate_to = action that folds into Attention.
- React = sink (paint); Event store/RxJS sessions = data stream, separate.
- TDD targets pure algebra first; live nav_path is conformance, not a substitute
  for unit red→green.

## Work queue (ONE coherent TDD increment per 5m firing)

Priority FAIL rows (from PARITY-HANDS — take the first still FAIL):
1. Optional: collapse board expandedKeys dual-inject (same pattern as scatter.brush — canvas sink already prefers attention$)
2. Facet chips as named entity (optional; query covers operational path) — remaining formal FAIL
3. Pixel wedge coords deferred (not a semantic node) — remaining formal FAIL
4. Do not redo PASS rows (CycleCard agentOpen, board expandKeys, scatterBrush paint + no dual-inject, per-apply, etc.)

Each firing:
1. Pick one FAIL row.
2. RED pure test → GREEN pure fold → sink/hash if needed → survey if control surface changed.
3. Update PARITY-HANDS only when land-asserted.
4. If you improve this loop, scheduler_create with task_id 019fa56901fd and the
   new prompt (do not delete+recreate).

## Gates
- Unit: `just test-attention`
- Live (if UI/control changed): `just nav-path` (needs :3002 + :5173)
- MCP if tools changed: `cd rs && cargo test -p open-story-mcp --lib`

## Exit criteria (report DONE; user may scheduler_delete)
1. PARITY-HANDS: no remaining FAIL for Story card interiors + canvas selection +
   explore filters without an explicit deferred row (with reason).
2. just test-attention green; just nav-path exit 0 (or env down documented).
3. navigate_to: event+expandAll; canvasMode+session; day; file; person — land OK.
4. Every PASS row in PARITY-HANDS has a named pure test that would fail without it.
5. No fetch inside foldIntent/foldControl.

## Anti-goals
- Playwright CSS as primary driver.
- Status report without RED→GREEN.
- Stopping early with "optional next" when FAIL rows remain and time remains.
- Mutating observed agent history.

## Key files
- docs/PARITY-HANDS.md
- ui/src/lib/attention.ts, nav-path.ts, ui-control.ts, hash-route.ts
- ui/src/streams/attention.ts, control.ts
- ui/src/App.tsx, components/story/*, components/canvas/*
- rs/mcp/src/tools/control.rs, agent-docs/hands.md
- scripts/nav_path.mjs
- ui/tests/lib/attention.test.ts, nav-path.test.ts, bdd.ts

## Env
- API http://127.0.0.1:3002  UI http://127.0.0.1:5173
```

---

## How to run / update the loop

**Scheduled (current):** task `019fa56901fd`, every **5m**, expires in 7 days.

Update without recreate:
```
scheduler_create(task_id="019fa56901fd", prompt=<new full prompt>, interval="5m")
```

Cancel:
```
scheduler_delete 019fa56901fd
```

**Manual:** copy the fenced PROMPT into a fresh agent / `/loop`.

---

## Status snapshot (2026-07-28) — **OPERATIONAL DONE**

| Level | Status |
|-------|--------|
| Operational exit criteria | **MET** — user may `scheduler_delete 019fa56901fd` |
| L1–L3 + Story interiors | PASS (expandAll, applyOpen, agentOpen) |
| Canvas selection / modes / brush / board | PASS (no dual-inject materialize) |
| Explore filters + facet chips | PASS (`kind:facet`) |
| Pixel wedge coords | FAIL deferred (non-semantic geometry — not Attention) |
| Gates | `just test-attention` green · `just nav-path` **29/29** |

If the loop still fires: re-verify gates only; do **not** invent new FAIL rows unless a real regression is red. Do not claim pixel wedge PASS.
