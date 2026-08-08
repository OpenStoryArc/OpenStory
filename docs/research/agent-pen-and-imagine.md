# Agent pen × Grok Imagine

**Status:** research note · 2026-08-08 · `feat/agent-pen`  
**Soul:** ink and generated assets stay on `ui.*` — never observed history.

## Two tools

| Verb | Role | Precision |
|------|------|-----------|
| **`draw`** | Geometry + recipes (edge-trace, stipple, paths) | Exact coords, TDD, replay |
| **Imagine** (`image_gen` / `image_edit`) | Raster appearance | Look / style; not exact structure |

## Real people (Max Glassie)

- Prefer **edge-trace / stipple** from a reference photo (honest “ink”).
- Or **Imagine `image_edit`** with that reference → stylized illustration, then place as texture *or* re-trace.
- Never bare `image_gen` for a named person without reference (Imagine skill).

## Recommended pipeline

```text
reference photo
  ├─► edge-trace + stipple  → draw strokes     (precision, default pen)
  └─► image_edit (ink style) → optional asset  → draw image OR re-trace
```

## Incorporation sketch

1. Keep `draw` for all geometry + recipes (`smiley`, `geometric-max`, `edge-portrait`).
2. Optional MCP/control `imagine` later: returns a short-lived data URL or path; agent then `draw`s it.
3. Three.js already in UI — future shader “ink wash” over a texture without leaving ui.*.
4. Persist with reels only as presentation overlay metadata, not events.

## Done on pen (this branch)

- Vector strokes + Draw tab + overlay
- **edge-portrait** recipe: Sobel paths + stipple (not photo paste)
- geometric Max fallback if CORS/load fails
