# Reels interaction matrix (problem space)

**Goal:** Cover combinations of slide kind × tools × ink so design stays simple and tested.

## Dimensions

| Dimension | Values |
|-----------|--------|
| **Slide kind** | title, diagram, spotlight, image |
| **Player** | play, pause, advance, back, jump segment |
| **Ink** | none, annotate on, mark, clear slide, re-annotate |
| **Actor** | human freehand, agent `draw target=slide` |
| **Eyes** | journey beatInk count, local store summary |

## Combinations (automated)

Pure (Vitest `reel-annotate.test.ts`):

- kind-agnostic storage: annotate/clear/re-annotate × 3 slides × 3 kinds  
- pause × jump isolation (ink does not merge across slides)  
- agent intent applyBeatInkIntent append/replace  

Live (`scripts/reels_interaction_matrix.py`):

- create multi-slide reel  
- agent ink each body-mapped slide  
- clear middle + re-ink  
- print journey beatInk review  

## Usability tasks (U1–U10)

See script `usability_checklist()` — run by human while agent drives, or reverse.

## Agent control (parity)

```json
{
  "action": "draw",
  "params": {
    "target": "slide",
    "reelId": "reel-…",
    "beatIndex": 1,
    "mode": "append",
    "strokes": [ { "type": "circle", "cx": 0.5, "cy": 0.5, "r": 0.08 } ]
  }
}
```

`beatIndex` = **unified slide index** (opener = 0 when present).
