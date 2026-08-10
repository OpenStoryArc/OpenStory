# Reel beat marginalia — 1:1 slide ink

**Status:** implement · 2026-08-10 · `feat/agent-pen`  
**Soul:** curation *about* a reading of history — not observed agent events.

## Vocabulary (avoid “event” for this)

| Term | Meaning |
|------|---------|
| **Beat** (preferred) / **slide** | One ordered unit in a reel narrative schema (`kind`: spotlight \| title \| diagram \| image + `line` + optional `visual`) |
| **Beat ink** / **marginalia** | Freehand (later: typed) strokes **owned by one beat** |
| **Observed event** | Real agent history CloudEvent — only spotlight beats *point at* these |

A reel is an ordered list of **beats** assembled under a narrative frame (e.g. BLUF opener → body beats → closer). Marginalia is a **parallel structure** keyed by beat identity, not a second global pen layer.

## Identity

```text
BeatKey = { reelId, beatIndex }   // stable for v1; later optional beatId UUID
```

Coordinates: **unit space of the stage** (same 0..1 as the pen / diagram stage for that slide). 1:1 with the beat’s visual frame — not the whole app chrome, not “whatever tab you’re on.”

## Store (v1)

Client-local (ui.* / browser):

```json
{
  "v": 1,
  "byKey": {
    "reel-abc:2": {
      "reelId": "reel-abc",
      "beatIndex": 2,
      "strokes": [ … ],
      "updatedAt": "…"
    }
  }
}
```

Key: `` `${reelId}:${beatIndex}` ``  
Later: optional merge into reel artifact or side-car JSON for portability.

**Not** written to `events.*` / coding-agent sessions.

## Runtime

```text
Playing reel R at beat i
  → stage shows beat i
  → ink layer shows only byKey[R:i]
  → Annotate on → freehand commits to byKey[R:i]
  → ADVANCE → switch ink to byKey[R:i+1] (previous marks stay with their beat)
```

Global `draw$` remains the **studio** canvas (Draw tab + free glass). Reel marginalia is a **separate map**.

## Journey eyes

Interactions may report:

```json
{
  "view": "reels",
  "reelId": "reel-…",
  "beatIndex": 2,
  "annotate": true,
  "beatInk": { "stroke_count": 3, … }
}
```

## Tests

- Pure: keying, append strokes, isolate beats, persist round-trip  
- Player wiring: active beat only (unit/integration as practical)
