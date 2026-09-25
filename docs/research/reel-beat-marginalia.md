# Reel beat marginalia — 1:1 slide ink

**Status:** implemented · 2026-08-10 · `feat/agent-pen` · ink-on-reel 2026-09-25 · `feat/reel-beat-ink-persist`  
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

## Store (v2, 2026-09-25) — ink lives on the reel

The localStorage store above is now a **cache**. The copy that travels is
on the reel record itself, in the same file the reel is saved in
(`{data_dir}/reels/{id}.json`), keyed by beat index:

```json
{
  "id": "reel-abc",
  "stops": [ … ],
  "beatInk": {
    "2": { "strokes": [ … ], "updatedAt": "…" }
  }
}
```

- **Write-through.** Every ink change on a beat (human pen, agent
  `commitBeatInkIntent`, clear) is `PUT /api/reels/{id}/ink/{beatIndex}`
  with that beat's full stroke list; empty strokes forget the beat. The
  server keeps strokes verbatim — it never interprets geometry.
- **Hydrate on fetch.** When the player fetches a reel, its `beatInk`
  replaces the cache for that reel (server wins). If the server has *no*
  ink for the reel but this browser does, the local beats are pushed up
  instead — ink drawn before v2 is migrated, not wiped.
- **Offline.** A failed PUT keeps the local copy; the next fetch reconciles.
- **Eyes.** An agent that wants to *see* the strokes reads the reel
  (`GET /api/reels/{id}` → `beatInk`) — the `beatInk` field in `ui-state`
  stays a light count/kind sample.

**Not** written to `events.*` / coding-agent sessions — a reel with ink is
still curation about history, never history.

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
