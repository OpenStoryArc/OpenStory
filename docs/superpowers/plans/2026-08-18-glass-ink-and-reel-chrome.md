# Glass Ink Scoping + Reel Chrome Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scope glass annotations to the view they were drawn on (extending the beat-ink law to the whole app), and remove the reel player's "unfinished" tells (caption echo, chip collisions/miscounts, destructive-red Done, off-palette diagrams).

**Architecture:** The branch already contains the correct scoping model — `lib/reel-annotate.ts` keys ink 1:1 to `reelId:beatIndex` ("not a global pen floating across the app"). This plan generalizes that model: a new string-keyed **glass-ink store** holds per-route-context annotations; the global `draw$` scene becomes the **board** (the Draw tab's document) and is no longer painted over other tabs. Reel chrome fixes are pure-function changes surfaced through existing components.

**Tech Stack:** React + RxJS BehaviorSubjects (existing stream pattern), Vitest for pure-lib specs, localStorage persistence (ui.* only), Tailwind v4 classes.

**Spec:** Design review conclusions from the 2026-08-18 session (this file's *Design decisions* section is the spec of record). Background: `docs/research/attention-canvas-v0.md` (v0.1 "one scene" law — superseded by this plan), `docs/research/reel-beat-marginalia.md` (the beat-ink precedent), `docs/research/reel-slide-standard.md`.

## Global Constraints

- Ink is `ui.*` only — never CloudEvents, never observed history. localStorage keys: board `openstory.draw.scene.v1` (unchanged), beat ink `openstory.reel.beat-ink.v1` (unchanged), glass ink **new** `openstory.glass-ink.v1`.
- Observe, never interfere — nothing here touches the Rust store or transcripts.
- TDD: pure logic lands in `ui/src/lib/` with a failing Vitest spec first; components stay thin sinks of streams.
- Board ink color stays `#2f4a3e`; human marginalia stays the dual-stroke (dark `#0f172a` understroke + `#facc15`) via `marginaliaPathStrokes` from `@/lib/pen-eyes`.
- Theme palette (dark): bg `#1a1b26`, surface `#24283b`, text `#cdd6f9`, accent `#7aa2f7`. Diagram beats must draw from this palette, not slate.
- Run `cd ui && npm test` per task; `just test-ui` before pushing.

## Design decisions (the spec)

1. **Two ink objects, not one.** The **board** is a document (the Draw tab paper): global, persistent, navigated *to*. **Glass ink** is annotation: deictic ("this, here"), so it is keyed by the context it points at and rendered only when that context is active. v0.1's "ink stays across tab navigation" is superseded — an arrow that follows you to a different page points at nothing.
2. **Context key = view + salient id.** `live`, `live:<sessionId>`, `story:<sessionId>`, `explore:<sessionId>`, `reels` (list), etc. The reels *player* slides keep their existing beat-ink store (`reelId:beatIndex`) untouched.
3. **Agent vocabulary grows a `scope`.** `draw` control intents accept `scope: "here" | "board"` (default `"here"`). "Here" resolves against the human's current route; on the Draw tab, "here" *is* the board — so existing agent flows ("draw this session on the board" with Draw open) work unchanged.
4. **Hide/Show-on-glass dies.** Its only job was mitigating mis-scoped ink. With scoping, the chip shows the *current context's* count and a per-context Clear.
5. **Caption bar never echoes a title stage.** `title`-kind slides render their line huge on the stage; the caption bar shows controls only.
6. **The ink chip yields to the player.** During reel slide playback the beat toolbar owns annotate; the floating chip unmounts (same rule `DrawOverlay` already applies via `activeBeatKey$`).
7. **Exiting annotate is not destructive.** Done buttons use accent styling, not rose.
8. **Diagram beats use the app's palette.** One ink + accent; no index-rotating rainbow; stage bg = `--bg` value, not slate-900. (The full hand-drawn "pen language" pass for diagrams is a follow-on branch — record in BACKLOG, do not build here.)

## File Structure

- `ui/src/lib/reel-slide.ts` — add `captionFor(slide)` (Task 1)
- `ui/src/lib/reel-visual.ts` — repalette `diagramLabelsToStrokes` (Task 3)
- `ui/src/lib/glass-ink.ts` — **new**: `routeGlassKey` + pure keyed store (Tasks 4–5)
- `ui/src/streams/glass-ink.ts` — **new**: BehaviorSubject wrapper, mirrors `streams/reel-annotate.ts` (Task 5)
- `ui/src/lib/ui-control.ts` — `resolveDrawScope` helper for the `draw` verb (Task 6)
- `ui/src/components/draw/DrawOverlay.tsx` — paint context ink, take `route` prop (Task 6)
- `ui/src/components/draw/DrawView.tsx` — drop Hide-on-glass UI (Task 6)
- `ui/src/components/draw/DrawInkChip.tsx` — yield during beats, context count (Tasks 2, 7)
- `ui/src/components/reels/ReelsView.tsx` — caption suppression, Done styling (Tasks 1–2)
- `ui/src/App.tsx` — pass `route` to overlay/chip; route draw intents by scope (Task 6)
- `ui/src/lib/hash-route.ts` — legacy `#view=X` tolerance (Task 8)
- Tests: `ui/tests/lib/reel-slide.test.ts`, `ui/tests/lib/reel-visual.test.ts`, `ui/tests/lib/glass-ink.test.ts` (new), `ui/tests/lib/ui-control.test.ts`, `ui/tests/lib/hash-route.test.ts`
- Docs: `docs/research/attention-canvas-v0.md` (v0.2 addendum, Task 6), `docs/BACKLOG.md` (pen-language diagrams follow-on, Task 3)

---

### Task 1: Caption bar stops echoing title slides

**Files:**
- Modify: `ui/src/lib/reel-slide.ts` (append near other pure helpers)
- Modify: `ui/src/components/reels/ReelsView.tsx:334` (caption `<p>`)
- Test: `ui/tests/lib/reel-slide.test.ts`

**Interfaces:**
- Produces: `captionFor(slide: Pick<Slide, "kind" | "line">): string | null` — null means "render no caption line".

- [ ] **Step 1: Write the failing test** (append to `ui/tests/lib/reel-slide.test.ts`)

```ts
import { captionFor } from "@/lib/reel-slide";

describe("captionFor", () => {
  it("suppresses the caption on title slides — the stage already shows the line", () => {
    expect(captionFor({ kind: "title", line: "Bottom line: it works." })).toBeNull();
  });

  it("returns the line for spotlight, diagram, and image slides", () => {
    expect(captionFor({ kind: "spotlight", line: "We searched first." })).toBe("We searched first.");
    expect(captionFor({ kind: "diagram", line: "The journey." })).toBe("The journey.");
    expect(captionFor({ kind: "image", line: "The screenshot." })).toBe("The screenshot.");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ui && npx vitest run tests/lib/reel-slide.test.ts`
Expected: FAIL — `captionFor` is not exported.

- [ ] **Step 3: Implement** (append to `ui/src/lib/reel-slide.ts`)

```ts
/** Caption line for the player's bottom bar — null when the stage already
 *  renders the line full-screen (title cards), so it is never shown twice. */
export function captionFor(slide: Pick<Slide, "kind" | "line">): string | null {
  return slide.kind === "title" ? null : slide.line;
}
```

- [ ] **Step 4: Use it in the player.** In `ReelsView.tsx`, inside the `slideChrome` block, replace the unconditional caption `<p>` with:

```tsx
{captionFor(slide) && (
  <p
    className="mx-auto max-w-4xl cursor-pointer text-center text-lg leading-relaxed text-[color:var(--text)]"
    onClick={() => !annotating && dispatch({ type: "ADVANCE" })}
  >
    {captionFor(slide)}
  </p>
)}
```

Import `captionFor` alongside the existing `normalizeReelToSlides` import. The controls row below stays unconditional.

- [ ] **Step 5: Run tests, verify pass**

Run: `cd ui && npm test`
Expected: PASS (including existing reel-player specs — the chrome renders, only the echo line is conditional).

- [ ] **Step 6: Commit**

```bash
git add ui/src/lib/reel-slide.ts ui/src/components/reels/ReelsView.tsx ui/tests/lib/reel-slide.test.ts
git commit -m "fix(reels): caption bar no longer echoes title slides verbatim"
```

---

### Task 2: Ink chip yields to the player; Done buttons stop looking destructive; Draw toolbar wraps

**Files:**
- Modify: `ui/src/components/draw/DrawInkChip.tsx`
- Modify: `ui/src/components/reels/ReelsView.tsx` ("Done annotating" classes)
- Modify: `ui/src/components/draw/DrawView.tsx:115` (toolbar wrap)

**Interfaces:**
- Consumes: `activeBeatKey$()` from `@/streams/reel-annotate` (same subscription `DrawOverlay.tsx:123` uses).

*(No new pure logic — there is no component-test infra in `ui/tests/`; verification is the full suite plus a dev-server look.)*

- [ ] **Step 1: Chip unmounts during beat playback.** In `DrawInkChip.tsx`:

```tsx
import { activeBeatKey$ } from "@/streams/reel-annotate";
// inside the component:
const [beatScoped, setBeatScoped] = useState(false);
// in the existing useEffect, add:
const c = activeBeatKey$().subscribe((k) => setBeatScoped(k != null));
// and unsubscribe c in the cleanup.
// before the return:
if (beatScoped) return null; // reel slide toolbar owns annotate on stage
```

- [ ] **Step 2: Done styling.** In `DrawInkChip.tsx` and `ReelsView.tsx`, replace the rose classes on the annotate toggles:

`border-rose-500 bg-rose-500/20 text-rose-700 dark:text-rose-200`
→ `border-[color:var(--accent)] bg-[color:var(--accent)]/15 text-[color:var(--accent)]`

(The yellow "ANNOTATING" banner already signals the mode; Done is a safe action and now matches the app's active-state idiom, e.g. the Pause button.)

- [ ] **Step 3: Draw toolbar wrap.** In `DrawView.tsx`, on the title `<div className="min-w-0 flex-1">` change to `className="min-w-0 basis-full lg:basis-auto lg:flex-1"` so at narrow widths the button cluster wraps to its own row instead of clipping off-screen.

- [ ] **Step 4: Run tests + eyeball**

Run: `cd ui && npm test` — expected PASS.
Then with `just dev` running: open a reel slide → no floating chip; open Draw at a narrow window → all buttons visible on a second row.

- [ ] **Step 5: Commit**

```bash
git add ui/src/components/draw/DrawInkChip.tsx ui/src/components/reels/ReelsView.tsx ui/src/components/draw/DrawView.tsx
git commit -m "fix(ui): ink chip yields during reel beats; Done uses accent not rose; Draw toolbar wraps"
```

---

### Task 3: Diagram beats speak the app's palette

**Files:**
- Modify: `ui/src/lib/reel-visual.ts:58-64` (color table), box width
- Modify: `ui/src/components/reels/ReelBeatStage.tsx:118,126` (stage bg `#0f172a` → `#1a1b26`)
- Modify: `docs/BACKLOG.md` (add follow-on entry)
- Test: `ui/tests/lib/reel-visual.test.ts`

**Interfaces:**
- Produces: `diagramLabelsToStrokes` unchanged signature; output boxes all stroke `#7aa2f7`, text `#cdd6f9`, connectors `#565f89`, box width fitted to label length.

- [ ] **Step 1: Write the failing test** (append to `ui/tests/lib/reel-visual.test.ts`)

```ts
describe("diagramLabelsToStrokes palette", () => {
  it("draws every box in the accent ink — color rotation encodes nothing and is gone", () => {
    const strokes = diagramLabelsToStrokes(["ToolSearch", "Bash ×3", "Edit"], { title: "Journey" });
    const boxes = strokes.filter((s) => s.type === "path");
    expect(boxes.length).toBe(3);
    for (const b of boxes) expect(b.stroke).toBe("#7aa2f7");
  });

  it("fits box width to the label instead of one wide bar", () => {
    const strokes = diagramLabelsToStrokes(["Bash", "a-much-longer-tool-label"]);
    const [short, long] = strokes.filter((s) => s.type === "path");
    const width = (p: { points: readonly { x: number }[] }) =>
      Math.max(...p.points.map((q) => q.x)) - Math.min(...p.points.map((q) => q.x));
    expect(width(short as never)).toBeLessThan(width(long as never));
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ui && npx vitest run tests/lib/reel-visual.test.ts`
Expected: FAIL — boxes rotate through `#93c5fd`/`#c4b5fd`/… and share one width.

- [ ] **Step 3: Implement.** In `diagramLabelsToStrokes`, delete the `colors` array and replace the per-item box/text/connector emission with:

```ts
const INK = "#7aa2f7";      // accent — one voice for agent diagrams
const TEXT = "#cdd6f9";     // theme text
const CONNECT = "#565f89";  // muted connector
for (let i = 0; i < items.length; i++) {
  const y = top + i * ((bottom - top) / items.length);
  const w = Math.min(0.64, Math.max(0.2, 0.05 + items[i]!.length * 0.013));
  const x = 0.5 - w / 2;
  strokes.push({
    type: "path",
    points: [
      { x, y },
      { x: x + w, y },
      { x: x + w, y: y + boxH },
      { x, y: y + boxH },
    ],
    closed: true,
    fill: "none",
    stroke: INK,
    strokeWidth: 2,
  });
  strokes.push({
    type: "text",
    x: 0.5,
    y: y + boxH / 2 + 0.01,
    text: items[i]!.slice(0, 36),
    fill: TEXT,
    fontSize: 18,
  });
  if (i < items.length - 1) {
    strokes.push({
      type: "line",
      x1: 0.5,
      y1: y + boxH,
      x2: 0.5,
      y2: y + (bottom - top) / items.length,
      stroke: CONNECT,
      strokeWidth: 2,
    });
  }
}
```

Title/empty-state text fills stay as they are but change `#e2e8f0` → `#cdd6f9` and `#94a3b8` → `#565f89`. In `ReelBeatStage.tsx` change both `#0f172a` stage fills to `#1a1b26`.

- [ ] **Step 4: Run tests, verify pass**

Run: `cd ui && npm test` — expected PASS.

- [ ] **Step 5: BACKLOG entry.** Add under the reels theme in `docs/BACKLOG.md`:

```markdown
- **Diagram beats in the pen's hand.** Diagram stops now use the theme palette,
  but they still read as wireframes. Render them in the agent pen's language —
  hand-drawn box strokes (RDP-simplified paths like the portrait recipes),
  single ink + accent, Georgia labels — so agent diagrams look drawn by the
  same pen that annotates them. The ink recipes from feat/agent-pen
  (draw-trace.ts, draw-portrait.ts) are the starting material.
```

- [ ] **Step 6: Commit**

```bash
git add ui/src/lib/reel-visual.ts ui/src/components/reels/ReelBeatStage.tsx ui/tests/lib/reel-visual.test.ts docs/BACKLOG.md
git commit -m "feat(reels): diagram beats use theme palette — one ink, fitted boxes"
```

---

### Task 4: `routeGlassKey` — pure context identity for glass ink

**Files:**
- Create: `ui/src/lib/glass-ink.ts`
- Test: `ui/tests/lib/glass-ink.test.ts` (new)

**Interfaces:**
- Consumes: `HashRoute` from `@/lib/hash-route`.
- Produces: `routeGlassKey(route: HashRoute): string | null` — null when another surface owns ink (Draw tab = board; reels player = beat store).

- [ ] **Step 1: Write the failing test** (`ui/tests/lib/glass-ink.test.ts`)

```ts
import { describe, expect, it } from "vitest";
import { routeGlassKey } from "@/lib/glass-ink";

describe("routeGlassKey", () => {
  it("is null on the Draw tab — the board owns that paper", () => {
    expect(routeGlassKey({ view: "draw" })).toBeNull();
  });

  it("is null inside the reels player — beat ink owns slides", () => {
    expect(routeGlassKey({ view: "reels", reelId: "reel-1" })).toBeNull();
  });

  it("keys the reels list, live, and per-session story contexts separately", () => {
    expect(routeGlassKey({ view: "reels" })).toBe("reels");
    expect(routeGlassKey({ view: "live" })).toBe("live");
    expect(routeGlassKey({ view: "live", sessionId: "abc" })).toBe("live:abc");
    expect(routeGlassKey({ view: "story", sessionId: "abc" })).toBe("story:abc");
    expect(routeGlassKey({ view: "explore", sessionId: "s1" })).toBe("explore:s1");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ui && npx vitest run tests/lib/glass-ink.test.ts`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement** (`ui/src/lib/glass-ink.ts`)

```ts
/**
 * Glass ink — per-context annotations on the mirror.
 *
 * Extends the beat-ink law (lib/reel-annotate.ts: "ink is 1:1 with a beat —
 * not a global pen floating across the app") to every view: annotation is
 * deictic, so it is keyed by what it points at and painted only while that
 * context is on screen. The Draw tab's board (draw$) stays a separate,
 * global document. Never observed history — ui.* / localStorage only.
 */

import type { HashRoute } from "@/lib/hash-route";

/** Context identity for glass ink, or null when another surface owns ink
 *  (Draw tab → board scene; reels player → beat-ink store). */
export function routeGlassKey(route: HashRoute): string | null {
  if (route.view === "draw") return null;
  if (route.view === "reels" && route.reelId) return null;
  return route.sessionId ? `${route.view}:${route.sessionId}` : route.view;
}
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cd ui && npx vitest run tests/lib/glass-ink.test.ts` — expected PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/lib/glass-ink.ts ui/tests/lib/glass-ink.test.ts
git commit -m "feat(ui): routeGlassKey — pure context identity for scoped glass ink"
```

---

### Task 5: Glass-ink keyed store (pure fns + stream)

**Files:**
- Modify: `ui/src/lib/glass-ink.ts` (store fns)
- Create: `ui/src/streams/glass-ink.ts`
- Test: `ui/tests/lib/glass-ink.test.ts`

**Interfaces:**
- Consumes: `DrawStroke`, `normalizeStrokes` from `@/lib/draw`.
- Produces (lib): `GlassInk { key, strokes, updatedAt }`, `GlassInkStore { v, byKey }`, `emptyGlassInkStore()`, `getGlassInk(store, key)`, `appendGlassInk(store, key, strokes, opts?)`, `clearGlassInk(store, key)`, `applyGlassInkIntent(store, {key, clear?, strokes?, mode?}, opts?)`, `pruneGlassInkStore(store, max)`, `normalizeGlassInkStore(raw)`, `loadGlassInkStore(storage?)`, `saveGlassInkStore(store, storage?)`, `GLASS_INK_STORAGE_KEY = "openstory.glass-ink.v1"`, `GLASS_INK_MAX_CONTEXTS = 40`.
- Produces (stream): `glassInkStore$()`, `getGlassInkFor(key)`, `appendGlassStrokes(key, strokes)`, `clearGlassContext(key)`, `commitGlassInkIntent({key, clear?, strokes?, mode?})`.

- [ ] **Step 1: Write the failing tests** (append to `ui/tests/lib/glass-ink.test.ts`)

```ts
import {
  appendGlassInk,
  applyGlassInkIntent,
  clearGlassInk,
  emptyGlassInkStore,
  getGlassInk,
  normalizeGlassInkStore,
  pruneGlassInkStore,
} from "@/lib/glass-ink";

const stroke = { type: "line", x1: 0, y1: 0, x2: 1, y2: 1 } as const;

describe("glass ink store", () => {
  it("appends strokes under one context without touching others", () => {
    let s = emptyGlassInkStore();
    s = appendGlassInk(s, "live", [stroke], { now: () => "t1" });
    s = appendGlassInk(s, "story:abc", [stroke, stroke], { now: () => "t2" });
    expect(getGlassInk(s, "live").strokes.length).toBe(1);
    expect(getGlassInk(s, "story:abc").strokes.length).toBe(2);
    expect(getGlassInk(s, "explore").strokes.length).toBe(0);
  });

  it("clear empties exactly one context", () => {
    let s = emptyGlassInkStore();
    s = appendGlassInk(s, "live", [stroke]);
    s = appendGlassInk(s, "reels", [stroke]);
    s = clearGlassInk(s, "live");
    expect(getGlassInk(s, "live").strokes.length).toBe(0);
    expect(getGlassInk(s, "reels").strokes.length).toBe(1);
  });

  it("replace intent overwrites; append (default) accumulates", () => {
    let s = emptyGlassInkStore();
    s = applyGlassInkIntent(s, { key: "live", strokes: [stroke, stroke] });
    s = applyGlassInkIntent(s, { key: "live", strokes: [stroke], mode: "replace" });
    expect(getGlassInk(s, "live").strokes.length).toBe(1);
  });

  it("prunes to the most recently updated contexts", () => {
    let s = emptyGlassInkStore();
    s = appendGlassInk(s, "a", [stroke], { now: () => "2026-01-01" });
    s = appendGlassInk(s, "b", [stroke], { now: () => "2026-01-02" });
    s = appendGlassInk(s, "c", [stroke], { now: () => "2026-01-03" });
    const pruned = pruneGlassInkStore(s, 2);
    expect(Object.keys(pruned.byKey).sort()).toEqual(["b", "c"]);
  });

  it("normalizes untrusted storage JSON (bad rows dropped, versions gated)", () => {
    expect(normalizeGlassInkStore(null).byKey).toEqual({});
    expect(normalizeGlassInkStore({ v: 99, byKey: {} }).byKey).toEqual({});
    const ok = normalizeGlassInkStore({
      v: 1,
      byKey: { live: { strokes: [stroke], updatedAt: "t" }, bad: 7 },
    });
    expect(Object.keys(ok.byKey)).toEqual(["live"]);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd ui && npx vitest run tests/lib/glass-ink.test.ts`
Expected: FAIL — store fns not exported.

- [ ] **Step 3: Implement the pure store** (append to `ui/src/lib/glass-ink.ts`; the shape deliberately mirrors `lib/reel-annotate.ts` so both read the same way)

```ts
import { normalizeStrokes, type DrawStroke } from "@/lib/draw";

export const GLASS_INK_STORAGE_KEY = "openstory.glass-ink.v1";
export const GLASS_INK_VERSION = 1;
/** Soft cap: keep the N most recently touched contexts. */
export const GLASS_INK_MAX_CONTEXTS = 40;

export type GlassInk = {
  readonly key: string;
  readonly strokes: readonly DrawStroke[];
  readonly updatedAt: string;
};

export type GlassInkStore = {
  readonly v: number;
  readonly byKey: Readonly<Record<string, GlassInk>>;
};

export function emptyGlassInkStore(): GlassInkStore {
  return { v: GLASS_INK_VERSION, byKey: {} };
}

export function getGlassInk(store: GlassInkStore, key: string): GlassInk {
  return store.byKey[key] ?? { key, strokes: [], updatedAt: "" };
}

export function setGlassInk(
  store: GlassInkStore,
  key: string,
  strokes: readonly DrawStroke[],
  opts?: { now?: () => string },
): GlassInkStore {
  const updatedAt = opts?.now?.() ?? new Date().toISOString();
  if (strokes.length === 0) {
    const { [key]: _drop, ...rest } = store.byKey;
    void _drop;
    return { v: GLASS_INK_VERSION, byKey: rest };
  }
  return {
    v: GLASS_INK_VERSION,
    byKey: { ...store.byKey, [key]: { key, strokes: [...strokes], updatedAt } },
  };
}

export function appendGlassInk(
  store: GlassInkStore,
  key: string,
  more: readonly DrawStroke[],
  opts?: { now?: () => string },
): GlassInkStore {
  if (more.length === 0) return store;
  const cur = getGlassInk(store, key);
  return setGlassInk(store, key, [...cur.strokes, ...more], opts);
}

export function clearGlassInk(store: GlassInkStore, key: string): GlassInkStore {
  return setGlassInk(store, key, []);
}

export function applyGlassInkIntent(
  store: GlassInkStore,
  intent: {
    readonly key: string;
    readonly clear?: boolean;
    readonly strokes?: readonly DrawStroke[];
    readonly mode?: "append" | "replace";
  },
  opts?: { now?: () => string },
): GlassInkStore {
  const strokes = intent.strokes ?? [];
  const replace = intent.clear === true || intent.mode === "replace";
  return replace
    ? setGlassInk(store, intent.key, strokes, opts)
    : appendGlassInk(store, intent.key, strokes, opts);
}

export function pruneGlassInkStore(store: GlassInkStore, max = GLASS_INK_MAX_CONTEXTS): GlassInkStore {
  const rows = Object.values(store.byKey);
  if (rows.length <= max) return store;
  const keep = [...rows]
    .sort((a, b) => (a.updatedAt < b.updatedAt ? 1 : -1))
    .slice(0, max);
  const byKey: Record<string, GlassInk> = {};
  for (const r of keep) byKey[r.key] = r;
  return { v: GLASS_INK_VERSION, byKey };
}

export function normalizeGlassInkStore(raw: unknown): GlassInkStore {
  if (!raw || typeof raw !== "object") return emptyGlassInkStore();
  const o = raw as Record<string, unknown>;
  if (o.v !== GLASS_INK_VERSION) return emptyGlassInkStore();
  const by = o.byKey;
  if (!by || typeof by !== "object") return emptyGlassInkStore();
  const out: Record<string, GlassInk> = {};
  for (const [k, v] of Object.entries(by as Record<string, unknown>)) {
    if (!k || !v || typeof v !== "object") continue;
    const row = v as Record<string, unknown>;
    const strokes = normalizeStrokes(row.strokes);
    if (strokes.length === 0) continue;
    out[k] = {
      key: k,
      strokes,
      updatedAt: typeof row.updatedAt === "string" ? row.updatedAt : "",
    };
  }
  return { v: GLASS_INK_VERSION, byKey: out };
}

export function loadGlassInkStore(
  storage: Pick<Storage, "getItem"> | null = typeof localStorage !== "undefined" ? localStorage : null,
): GlassInkStore {
  if (!storage) return emptyGlassInkStore();
  try {
    const text = storage.getItem(GLASS_INK_STORAGE_KEY);
    if (!text) return emptyGlassInkStore();
    return normalizeGlassInkStore(JSON.parse(text) as unknown);
  } catch {
    return emptyGlassInkStore();
  }
}

export function saveGlassInkStore(
  store: GlassInkStore,
  storage: Pick<Storage, "setItem" | "removeItem"> | null = typeof localStorage !== "undefined"
    ? localStorage
    : null,
): boolean {
  if (!storage) return false;
  try {
    if (Object.keys(store.byKey).length === 0) {
      storage.removeItem(GLASS_INK_STORAGE_KEY);
      return true;
    }
    storage.setItem(GLASS_INK_STORAGE_KEY, JSON.stringify(pruneGlassInkStore(store)));
    return true;
  } catch {
    return false;
  }
}
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cd ui && npx vitest run tests/lib/glass-ink.test.ts` — expected PASS.

- [ ] **Step 5: Create the stream** (`ui/src/streams/glass-ink.ts` — same pattern as `streams/reel-annotate.ts`)

```ts
/**
 * Glass-ink stream — per-context annotations, keyed by routeGlassKey.
 */

import { BehaviorSubject, type Observable } from "rxjs";
import { normalizeStrokes, type DrawStroke } from "@/lib/draw";
import {
  appendGlassInk,
  applyGlassInkIntent,
  clearGlassInk,
  getGlassInk,
  loadGlassInkStore,
  saveGlassInkStore,
  type GlassInk,
  type GlassInkStore,
} from "@/lib/glass-ink";

const store$ = new BehaviorSubject<GlassInkStore>(loadGlassInkStore());

function persist(next: GlassInkStore): void {
  store$.next(next);
  saveGlassInkStore(next);
}

export function glassInkStore$(): Observable<GlassInkStore> {
  return store$.asObservable();
}

export function getGlassInkFor(key: string): GlassInk {
  return getGlassInk(store$.value, key);
}

export function appendGlassStrokes(key: string, strokes: readonly DrawStroke[]): GlassInk {
  persist(appendGlassInk(store$.value, key, strokes));
  return getGlassInk(store$.value, key);
}

export function clearGlassContext(key: string): void {
  persist(clearGlassInk(store$.value, key));
}

export function commitGlassInkIntent(intent: {
  key: string;
  clear?: boolean;
  strokes?: readonly unknown[];
  mode?: "append" | "replace";
}): GlassInk {
  const strokes = intent.strokes ? normalizeStrokes(intent.strokes) : [];
  persist(
    applyGlassInkIntent(store$.value, {
      key: intent.key,
      clear: intent.clear,
      strokes,
      mode: intent.mode,
    }),
  );
  return getGlassInk(store$.value, intent.key);
}
```

- [ ] **Step 6: Run the full UI suite**

Run: `cd ui && npm test` — expected PASS.

- [ ] **Step 7: Commit**

```bash
git add ui/src/lib/glass-ink.ts ui/src/streams/glass-ink.ts ui/tests/lib/glass-ink.test.ts
git commit -m "feat(ui): glass-ink keyed store — per-context annotations with LRU prune"
```

---

### Task 6: Overlay paints the current context; board stays on its paper

**Files:**
- Modify: `ui/src/lib/ui-control.ts` (scope resolver)
- Modify: `ui/src/components/draw/DrawOverlay.tsx`
- Modify: `ui/src/components/draw/DrawView.tsx` (drop Hide-on-glass UI)
- Modify: `ui/src/components/draw/DrawInkChip.tsx` (drop Show/Hide buttons)
- Modify: `ui/src/App.tsx` (pass `route`; route draw intents by scope)
- Modify: `docs/research/attention-canvas-v0.md` (v0.2 addendum)
- Test: `ui/tests/lib/ui-control.test.ts`

**Interfaces:**
- Consumes: `routeGlassKey` (Task 4), `appendGlassStrokes` / `commitGlassInkIntent` / `glassInkStore$` (Task 5), `marginaliaPathStrokes` from `@/lib/pen-eyes`, `commitDraw` from `@/streams/draw`, `commitBeatInkIntent` from `@/streams/reel-annotate`.
- Produces: `resolveDrawScope(params: ControlParams, glassKey: string | null): { target: "board" } | { target: "glass"; key: string } | { target: "beat"; reelId: string; beatIndex: number }` exported from `ui/src/lib/ui-control.ts`.

- [ ] **Step 1: Write the failing test** (append to `ui/tests/lib/ui-control.test.ts`)

```ts
import { resolveDrawScope } from "@/lib/ui-control";

describe("resolveDrawScope", () => {
  it("scope board → board, regardless of where the human is", () => {
    expect(resolveDrawScope({ scope: "board" }, "live")).toEqual({ target: "board" });
  });

  it("default (here) → the human's current glass context", () => {
    expect(resolveDrawScope({}, "story:abc")).toEqual({ target: "glass", key: "story:abc" });
  });

  it("here with no glass context (Draw tab open) falls back to the board", () => {
    expect(resolveDrawScope({}, null)).toEqual({ target: "board" });
  });

  it("explicit beat params → beat ink", () => {
    expect(resolveDrawScope({ reelId: "r1", beatIndex: 2 }, "reels")).toEqual({
      target: "beat",
      reelId: "r1",
      beatIndex: 2,
    });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ui && npx vitest run tests/lib/ui-control.test.ts`
Expected: FAIL — `resolveDrawScope` not exported.

- [ ] **Step 3: Implement the resolver** (in `ui/src/lib/ui-control.ts`, near the draw-verb handling)

```ts
/** Where a draw intent lands. "here" (default) = the human's current glass
 *  context; on the Draw tab "here" IS the board, so agent flows that open
 *  Draw and then ink keep working unchanged. */
export function resolveDrawScope(
  params: ControlParams,
  glassKey: string | null,
):
  | { target: "board" }
  | { target: "glass"; key: string }
  | { target: "beat"; reelId: string; beatIndex: number } {
  if (typeof params.reelId === "string" && typeof params.beatIndex === "number") {
    return { target: "beat", reelId: params.reelId, beatIndex: params.beatIndex };
  }
  if (params.scope === "board") return { target: "board" };
  if (glassKey) return { target: "glass", key: glassKey };
  return { target: "board" };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd ui && npx vitest run tests/lib/ui-control.test.ts` — expected PASS.

- [ ] **Step 5: Rewire `DrawOverlay`.** It gains a `route: HashRoute` prop (App already holds `route` at `App.tsx:79`). Changes inside the component:
  - `const glassKey = routeGlassKey(route);`
  - Replace the `drawScene$()` subscription with `glassInkStore$()`; derive `strokes` = `store.byKey[glassKey]?.strokes ?? []` (empty when `glassKey` is null).
  - Render nothing when `glassKey == null && !interactive` (board is never projected; beat layer still handled by the existing `beatScoped` early-return).
  - `endStroke` commits via `appendGlassStrokes(glassKey, marginaliaPathStrokes(pts))` instead of `commitDraw`.
  - Add `data-glass-key={glassKey ?? ""}` next to the existing testid for e2e/agent eyes.
  - The `scene.visible` check disappears (visibility was the old mitigation; context scoping replaces it).

- [ ] **Step 6: Rewire the draw verb in `App.tsx`.** Where the control applier handles the `"draw"` action, route by scope:

```ts
const scope = resolveDrawScope(action.params ?? {}, routeGlassKey(routeRef.current));
if (scope.target === "board") commitDraw(action);
else if (scope.target === "beat")
  commitBeatInkIntent({ reelId: scope.reelId, beatIndex: scope.beatIndex, clear: action.clear, strokes: action.strokes, mode: action.mode });
else commitGlassInkIntent({ key: scope.key, clear: action.clear, strokes: action.strokes, mode: action.mode });
```

(Adapt field names to the actual `UIControlAction` draw variant — it carries `clear`/`strokes` per `ui-control.ts:371-416`; thread `params` through the action if not already present.)

- [ ] **Step 7: Remove the Hide/Show mitigation.** In `DrawView.tsx`: delete the "Hide on glass"/"Show on glass" button, the amber "Overlay hidden" banner, and the `hidden` chip-suffix. In `DrawInkChip.tsx`: delete the Show/Hide button. Keep `setDrawVisible` exported (board-internal; MCP `visible` intents still parse) but no UI calls it.

- [ ] **Step 8: Doc addendum.** Append to `docs/research/attention-canvas-v0.md`:

```markdown
## v0.2 — scoped glass (2026-08-18)

The v0.1 "one scene" law is superseded. Annotation is deictic — ink points at
what is on screen — so glass ink is keyed by route context
(`lib/glass-ink.ts: routeGlassKey`, store `openstory.glass-ink.v1`) and painted
only while that context is active. The Draw tab's paper remains the one global
**board** (`draw$`, `openstory.draw.scene.v1`), a document you navigate to; it
is no longer projected over other tabs, and Hide-on-glass is gone (scoping made
it unnecessary). Reel slides keep beat ink (`reelId:beatIndex`) unchanged.
Agent draw intents take `scope: "here" | "board"` (default "here"); explicit
`reelId + beatIndex` still targets a slide.
```

- [ ] **Step 9: Full suite + eyeball**

Run: `cd ui && npm test` — expected PASS.
With `just dev`: annotate over Live → navigate to Story → ink gone; back to Live → ink returns. Draw tab paper unaffected either way.

- [ ] **Step 10: Commit**

```bash
git add ui/src/lib/ui-control.ts ui/src/components/draw/DrawOverlay.tsx ui/src/components/draw/DrawView.tsx ui/src/components/draw/DrawInkChip.tsx ui/src/App.tsx ui/tests/lib/ui-control.test.ts docs/research/attention-canvas-v0.md
git commit -m "feat(ui): glass ink scoped to route context — board stays on the Draw paper"
```

---

### Task 7: Chip shows the current context's ink

**Files:**
- Modify: `ui/src/components/draw/DrawInkChip.tsx` (needs `route` prop from App)
- Modify: `ui/src/App.tsx` (pass `route` to the chip)

**Interfaces:**
- Consumes: `routeGlassKey`, `glassInkStore$`, `clearGlassContext` (Tasks 4–5).

- [ ] **Step 1: Rewire the chip.** Replace the `drawScene$` subscription with `glassInkStore$` + `routeGlassKey(route)`:
  - Count shown = current context's stroke count: `✎ 3 here` (label the count "here", not a global total — copy matches the scoping model).
  - Clear button → `clearGlassContext(key)`, titled "Clear ink on this view".
  - "Draw" button (opens the board) renames to "Board" — it navigates to a different document now, and the label should say so.
  - When `routeGlassKey(route)` is null (Draw tab or reel slide) the chip renders nothing (Task 2 already handles the beat case).

- [ ] **Step 2: Agent eyes for glass strokes.** In `DrawOverlay.endStroke` (Task 6), after appending, mirror `BeatInkLayer.reportBeatInk` (`BeatInkLayer.tsx:78`) with the interaction post:

```ts
postInteraction({
  kind: "navigate",
  view: route.view,
  glassInk: { key: glassKey, stroke_count: getGlassInkFor(glassKey).strokes.length },
});
```

(`postInteraction` from `@/lib/interaction`, same import path BeatInkLayer uses.)

- [ ] **Step 3: Full suite + eyeball**

Run: `cd ui && npm test` — expected PASS. In dev: chip count changes when switching tabs; Clear only empties the visible view's ink.

- [ ] **Step 4: Commit**

```bash
git add ui/src/components/draw/DrawInkChip.tsx ui/src/components/draw/DrawOverlay.tsx ui/src/App.tsx
git commit -m "feat(ui): ink chip reflects current glass context — count, clear, Board"
```

---

### Task 8: Legacy hash tolerance (`#view=draw` → `#/draw`)

**Files:**
- Modify: `ui/src/lib/hash-route.ts` (`parseHash`)
- Test: `ui/tests/lib/hash-route.test.ts`

**Interfaces:**
- Produces: `parseHash` additionally accepts the query-style `#view=<name>` form agents and older docs emit, mapping it to the same `HashRoute` as `#/<name>`. Unknown views still fall back to `live`.

- [ ] **Step 1: Write the failing test** (append to `ui/tests/lib/hash-route.test.ts`)

```ts
describe("legacy #view= hashes", () => {
  it("maps #view=draw to the draw route instead of silently landing on Live", () => {
    expect(parseHash("#view=draw").view).toBe("draw");
    expect(parseHash("#view=reels").view).toBe("reels");
  });

  it("still falls back to live for unknown views", () => {
    expect(parseHash("#view=nonsense").view).toBe("live");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ui && npx vitest run tests/lib/hash-route.test.ts` — expected FAIL (both parse to `live` today, so the first spec fails).

- [ ] **Step 3: Implement.** At the top of `parseHash`, before the `#/` path parsing, translate the legacy form:

```ts
const legacy = /^#?view=([a-z]+)$/.exec(hash.trim());
if (legacy) return parseHash(`#/${legacy[1]}`);
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cd ui && npm test` — expected PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/src/lib/hash-route.ts ui/tests/lib/hash-route.test.ts
git commit -m "fix(ui): accept legacy #view=X hashes instead of silently routing to Live"
```

---

## Verification (whole plan)

- [ ] `just test` (Rust + UI, mirrors CI) — all green.
- [ ] `grep -rn "draw-overlay\|draw-ink-chip" e2e/tests/` — no e2e specs reference these testids today (verified 2026-08-18); if any appeared since, update their assumptions about global ink.
- [ ] Dev-server walkthrough: (1) annotate on Live → switch to Story → glass is clean → back to Live → ink returns; (2) reel slide annotate unchanged, chip absent; (3) title slides show no caption echo; (4) diagram beat renders in accent ink on `#1a1b26`; (5) Draw board unaffected by all of the above.
- [ ] `python3 scripts/check_docs.py` — docs still consistent after the attention-canvas addendum.

## Out of scope (recorded, not built)

- Hand-drawn pen-language diagram beats — BACKLOG entry added in Task 3.
- Spotlight beats rendering raw JSON — already in BACKLOG (Event Spotlight readability).
- Unifying the beat-ink store into the glass-ink store (`reel:<id>:<beat>` keys) — possible later migration; not worth the regression risk while both stores are small.
- Reel list beat-kind glyphs and idle-screen stop preview — small, but new feature surface; propose separately.
