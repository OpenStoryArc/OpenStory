# Reel slide standard (simple)

**Status:** standardize · 2026-08-10  
**Soul:** slides are curation *about* history; ink is parallel, 1:1 per slide.

---

## How annotations are saved *today* (honest)

| What | Where |
|------|--------|
| **Reel definition** (title, opener, stops, closer) | Server: `data/reels/{reelId}.json` |
| **Slide ink / marginalia** | Browser only: `localStorage` key `openstory.reel.beat-ink.v1` |
| **Key** | `` `${reelId}:${beatIndex}` `` (body stop index — **not** opener/closer yet) |
| **Near reels on disk?** | **No** — not co-located with reel JSON yet |
| **Agent eyes** | Journey: `beatInk: { stroke_count, kinds, beatIndex }` — **counts**, not full geometry |

So: related **by key**, not a shared file. Portable reel ≠ portable ink (yet).

---

## Single format (target)

One list: **`slides[]`**. No separate opener/closer/stops mental model for tools or UI.

```ts
type SlideKind = "title" | "diagram" | "spotlight" | "image";

interface Slide {
  id: string;              // stable within reel, e.g. "s0"
  kind: SlideKind;
  /** Spoken + caption — written for the ear (BLUF line, beat, closer). */
  line: string;
  /** Optional pin into real history (spotlight; optional elsewhere). */
  anchor?: { sessionId: string; eventId: string; clipAt?: string };
  /** Stage content (not the chrome). */
  visual?: {
    title?: string;
    labels?: string[];     // diagram nodes
    imageHref?: string;
    sessionId?: string;    // live tool-journey source
  };
  /** Parallel layer — same stage coords (0..1). */
  ink?: {
    strokes: DrawStroke[];
    updatedAt?: string;
  };
}
```

```ts
interface ReelDoc {
  id: string;
  title: string;
  created: string;
  author: string;
  /** Narrative frame — optional metadata only; body is slides. */
  framework?: "bluf" | "kishotenketsu" | "abt" | "free";
  slides: Slide[];
}
```

**BLUF as framework:** first slide is often `kind: "title"` with the BLUF line; last may be title closer. No special-case phases required.

**Tools over slides:** one toolbar on every slide:

```text
[ ‹ ] [ ▶/⏸ ] [ ● ● ● ] [ › ]   [ ✎ Annotate ] [ Clear ink ]
```

**Look:** one stage (full viewport), one caption bar, one ink layer keyed by `slide.id`.

---

## Wireframe

```text
┌─────────────────────────────────────────────────────────────┐
│  STAGE (full viewport)                                      │
│                                                             │
│     ┌─────────────────────────────────────────┐             │
│     │  visual by kind:                        │             │
│     │   title     → big centered line         │             │
│     │   diagram   → layered boxes/labels      │             │
│     │   spotlight → event card                │             │
│     │   image     → still                     │             │
│     │                                         │             │
│     │  + ink overlay (unit 0..1, this slide)  │             │
│     └─────────────────────────────────────────┘             │
│                                                             │
│  [banner if annotating: "Slide 2 ink — yellow"]             │
├─────────────────────────────────────────────────────────────┤
│  CAPTION / TOOLS (always same)                              │
│  "Narration line…"                                          │
│  [‹] [▶ Play|⏸ Pause] [●─●─○] [›]  [✎ Annotate] [Clear]  2/5 │
└─────────────────────────────────────────────────────────────┘
```

**Idle (not playing):** reel list only — title, slide count, Play.  
**Studio (Draw tab):** separate global pen — not slide ink.

---

## Storage target (related, simple)

```text
data/reels/{id}.json          # ReelDoc (slides include optional ink when saved)
# OR side-car (if ink stays client-first):
data/reels/{id}.ink.json      # { bySlideId: { s0: { strokes, updatedAt } } }
```

v1 path we take now in code: **normalize to `Slide[]` in the UI**; disk still accepts legacy opener/stops/closer and maps 1:1 into slides. Ink remains localStorage keyed by `reelId:slideIndex` until we write side-car/server.

---

## Mapping (legacy → standard)

| Legacy | Slide |
|--------|--------|
| `opener` | `slides[0]` kind `title`, line = opener |
| `stops[i]` | `slides[i+offset]` kind from stop.kind |
| `closer` | `slides[last]` kind `title`, line = closer |
| beat ink index | same index in unified `slides[]` |

---

## What agents can interpret today

- **Counts & which slide** — yes (`stroke_count`, `beatIndex`, `kinds: { path: N }`)  
- **What you drew** (shapes/meaning) — **no** (geometry not on server)  
- **Full reconstruct** — needs strokes in journey or reel file  

Next step for “I can read your circles”: include capped stroke sample in `beatInk` wire or persist ink next to reel.
