# Attention canvas v0.1 — first harden

**Status:** design + implement on `feat/agent-pen` · 2026-08-09  
**Soul:** ink is `ui.*` only — a shared attention surface, never observed history.

## Goal (this slice)

Solidify the **playable attention canvas** before more diagram libraries / reports:

1. **Ink stays** — across tab navigation and full page reload (client local).
2. **Canvas is present** — Draw tab is a first-class paper; overlay carries the same scene.
3. **Navigate freely** — history tabs remain usable under/over ink (hide without clear).
4. **Marginalia** — human freehand appends; agent strokes replace/append via control; Clear is explicit.

Not in v0.1: multi-board documents, HTML reports, Mermaid-in-pen, scroll-sticky pins.

**Reel slides:** marginalia is **not** the global pen — see `reel-beat-marginalia.md`
(beat-scoped ink 1:1 with `reelId + beatIndex`).

## Model

```text
draw$  (one global DrawScene)
  ├── Draw tab     — interactive paper (grid, freehand, Clear / Hide)
  ├── Draw overlay — same ink on Live/Story/Explore/… (pointer-events: none)
  ├── localStorage — hydrate/persist scene (browser-local, not events.*)
  └── pen eyes     — debounced ui-state snapshot for agents
```

| Field | Meaning |
|-------|---------|
| `strokes` | Geometry (path, line, text, …) in unit viewport space |
| `visible` | Overlay/tab still *have* ink; hidden = not painted (navigate without clear) |
| `label` | Soft attribution (human / issuer) |

## Laws

1. **Never history** — no CloudEvents for pen content; localStorage + ui-state only.
2. **One scene** — not a stack of boards (yet).
3. **Clear ≠ hide** — Hide keeps strokes; Clear empties.
4. **Human appends** — freehand does not wipe agent diagrams unless Clear.
5. **Agent may replace** — `draw` with `clear`/`recipe` is intentional.

## UX chrome (v0.1)

- **Draw tab:** paper + “N strokes · Hide/Show · Clear”
- **Other tabs:** floating ink chip when strokes exist — Open Draw · Hide · Clear
- No recipe sticker buttons (control seam only)

## Persistence

- Key: `openstory.draw.scene.v1`
- Cap strokes on save (same spirit as pen-eyes caps) so storage stays bounded
- Corrupt / oversize → empty scene

## Done when

- [x] Design note
- [x] Reload keeps last board (`openstory.draw.scene.v1`)
- [x] Navigate Story ↔ Draw without losing ink (global `draw$`)
- [x] Hide ink, read history, Show again (`visible` + chip)
- [x] Marginalia freehand without text-selection fights
- [x] Tests for persist hydrate
- [x] Draw chrome + floating ink chip on non-Draw tabs
