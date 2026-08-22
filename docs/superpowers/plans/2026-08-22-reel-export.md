# Reel Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export a reel as a single self-contained `.reel.html` file — preview + scan gated — with the ReelBundle JSON as the embedded, video-ready contract.

**Architecture:** Pure modules build a `ReelBundle` (pre-resolved slides, sanitized spotlight snapshots, inlined images, beat ink, scan receipt); `bakeReelHtml` renders it into one offline HTML document with the bundle embedded as `<script type="application/json" id="reel-bundle">`; a dialog in the Reels UI runs collect → preview (iframe srcdoc) → scan → download. The future video renderer is a second consumer of the same bundle schema.

**Tech Stack:** React + RxJS UI (existing), Vitest (`ui/tests/lib` pure + `ui/tests/components` jsdom), TypeScript strict, no server changes.

**Spec:** `docs/superpowers/specs/2026-08-22-reel-export-design.md` — the binding authority; read it first. The schema, gate semantics, and error-handling table there are requirements, not suggestions.

## Global Constraints

- No server/Rust changes. No new CloudEvents — export reads history and authored ink only.
- `exportedBy` is a display name, never an email.
- The exported document makes zero network requests: no `http(s)://` URLs in src/href, no external fonts/scripts. Data URIs and inline content only.
- Snapshot sanitizer is allowlist-based over a DOMParser tree — never regex-over-HTML.
- Storage keys and existing stores are read-only to this feature (`openstory.reel.beat-ink.v1` read for ink; nothing written).
- TDD: every pure module gets its failing spec first; keep RED/GREEN evidence in reports.
- Suite invocations: focused `cd ui && npx vitest run tests/lib/<file>.test.ts`; full `cd ui && npm test` once before each commit; `npx tsc --noEmit` clean before each commit.
- Existing helpers to reuse (do not reimplement): `normalizeReelToSlides`, `captionFor` (`@/lib/reel-slide`); `DrawStroke`, `normalizeStrokes`, `pathToSvgD` (`@/lib/draw`); `fetchReel` (`@/lib/reels-api`); `getBeatInk`, `loadBeatInkStore` (`@/lib/reel-annotate`).
- Commit messages follow repo convention; append trailer:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## File Structure

- Create: `ui/src/lib/reel-bundle.ts` — schema types, `buildBundle`, `bundleText`
- Create: `ui/src/lib/export-sanitize.ts` — `sanitizeSnapshotHtml`
- Create: `ui/src/lib/export-scan.ts` — `scanBundle`, `Finding`
- Create: `ui/src/lib/export-template.ts` — `bakeReelHtml`
- Create: `ui/src/lib/export-collect.ts` — side-effectful collector
- Create: `ui/src/components/reels/ExportReelDialog.tsx`
- Modify: `ui/src/components/reels/ReelsView.tsx` — Export affordances (list row + player idle)
- Tests: `ui/tests/lib/reel-bundle.test.ts`, `ui/tests/lib/export-sanitize.test.ts`, `ui/tests/lib/export-scan.test.ts`, `ui/tests/lib/export-template.test.ts`, `ui/tests/components/ExportReelDialog.test.tsx`
- Docs: `docs/BACKLOG.md` (video consumer entry), `docs/agent-in-ui.md` (export = human affordance note)

---

### Task 1: `reel-bundle.ts` — schema + builder + text flattener

**Files:**
- Create: `ui/src/lib/reel-bundle.ts`
- Test: `ui/tests/lib/reel-bundle.test.ts`

**Interfaces:**
- Consumes: `Slide`, `normalizeReelToSlides`, `captionFor` from `@/lib/reel-slide`; `Reel` from `@/lib/reels-api`; `DrawStroke` from `@/lib/draw`.
- Produces (later tasks depend on these exact names): types `ReelBundle`, `BundleSlide`, `BundleStage`, `ScanReceipt`; `buildBundle(reel: Reel, stages: ReadonlyMap<string, BundleStage>, ink: ReadonlyMap<string, readonly DrawStroke[]>, meta: { exportedBy: string; now?: () => string }): ReelBundle`; `bundleText(bundle: ReelBundle): { slideId: string; field: string; text: string }[]`.

- [ ] **Step 1: Write the failing tests** (`ui/tests/lib/reel-bundle.test.ts`)

```ts
import { describe, expect, it } from "vitest";
import { buildBundle, bundleText, type BundleStage } from "@/lib/reel-bundle";
import type { Reel } from "@/lib/reels-api";

const REEL: Reel = {
  id: "r1",
  title: "T",
  author: "max",
  created: "2026-08-01T00:00:00Z",
  opener: "The bottom line.",
  closer: "The end.",
  stops: [
    { line: "A spotlight.", kind: "spotlight", sessionId: "s1", eventId: "e1" },
    { line: "A diagram.", kind: "diagram", visual: { labels: ["Bash"] } },
  ],
} as Reel;

const stages = new Map<string, BundleStage>([
  ["s1", { type: "snapshot", html: "<div class=\"card\">hi</div>" }],
]);

describe("buildBundle", () => {
  it("folds opener/closer into pre-resolved slides with roles and captions", () => {
    const b = buildBundle(REEL, stages, new Map(), { exportedBy: "max", now: () => "t0" });
    expect(b.v).toBe(1);
    expect(b.kind).toBe("openstory.reel-bundle");
    expect(b.reel.slides.map((s) => s.role)).toEqual(["opener", "body", "body", "closer"]);
    // title-kind slides (opener/closer render as title cards) get caption: null
    expect(b.reel.slides[0]!.caption).toBeNull();
    expect(b.reel.slides[1]!.caption).toBe("A spotlight.");
  });

  it("keeps the spotlight anchor and attaches its snapshot stage by slide id", () => {
    const b = buildBundle(REEL, stages, new Map(), { exportedBy: "max" });
    const spot = b.reel.slides.find((s) => s.kind === "spotlight")!;
    expect(spot.anchor).toEqual({ sessionId: "s1", eventId: "e1" });
    expect(spot.stage).toEqual({ type: "snapshot", html: "<div class=\"card\">hi</div>" });
  });

  it("downgrades a spotlight with no captured stage to a text stage (visible, not silent)", () => {
    const b = buildBundle(REEL, new Map(), new Map(), { exportedBy: "max" });
    const spot = b.reel.slides.find((s) => s.kind === "spotlight")!;
    expect(spot.stage).toEqual({ type: "text" });
  });

  it("carries beat ink keyed by slide id and never invents ink", () => {
    const withInk = new Map([["s1", [{ type: "line", x1: 0, y1: 0, x2: 1, y2: 1 }] as const]]);
    const b = buildBundle(REEL, stages, withInk, { exportedBy: "max" });
    const inked = b.reel.slides.filter((s) => s.ink);
    expect(inked.length).toBe(1);
  });

  it("round-trips through JSON unchanged", () => {
    const b = buildBundle(REEL, stages, new Map(), { exportedBy: "max", now: () => "t0" });
    expect(JSON.parse(JSON.stringify(b))).toEqual(b);
  });
});

describe("bundleText", () => {
  it("flattens lines and snapshot text content for the scanner", () => {
    const b = buildBundle(REEL, stages, new Map(), { exportedBy: "max" });
    const rows = bundleText(b);
    expect(rows.some((r) => r.field === "line" && r.text === "A spotlight.")).toBe(true);
    expect(rows.some((r) => r.field === "snapshot" && r.text.includes("hi"))).toBe(true);
  });
});
```

- [ ] **Step 2: Run to verify RED** — `cd ui && npx vitest run tests/lib/reel-bundle.test.ts` — expected: module not found.

- [ ] **Step 3: Implement** (`ui/src/lib/reel-bundle.ts`)

```ts
/**
 * ReelBundle — the export contract. The HTML artifact is its first renderer,
 * the video pipeline (docs/research/reel-to-video.md) its second. Pure: the
 * collector supplies stages/ink; this module only assembles and flattens.
 */

import type { Reel } from "@/lib/reels-api";
import { captionFor, normalizeReelToSlides, type Slide } from "@/lib/reel-slide";
import type { DrawStroke } from "@/lib/draw";

export type BundleStage =
  | { readonly type: "text" }
  | { readonly type: "strokes"; readonly strokes: readonly DrawStroke[] }
  | { readonly type: "image"; readonly dataUri: string }
  | { readonly type: "snapshot"; readonly html: string };

export interface ScanReceipt {
  readonly v: 1;
  readonly findings: number;
  readonly acknowledged: boolean;
}

export interface BundleSlide {
  readonly id: string;
  readonly kind: Slide["kind"];
  readonly role: "opener" | "body" | "closer";
  readonly line: string;
  readonly caption: string | null;
  readonly anchor?: { readonly sessionId: string; readonly eventId: string };
  readonly stage: BundleStage;
  readonly ink?: readonly DrawStroke[];
}

export interface ReelBundle {
  readonly v: 1;
  readonly kind: "openstory.reel-bundle";
  readonly exportedAt: string;
  readonly exportedBy: string;
  readonly reel: {
    readonly id: string;
    readonly title: string;
    readonly author: string;
    readonly created: string;
    readonly slides: readonly BundleSlide[];
  };
  readonly scan: ScanReceipt;
}

/** Default stage when the collector captured nothing for a slide: the line
 *  itself, rendered as text — visible downgrade per the spec's error table. */
function defaultStage(slide: Slide): BundleStage {
  if (slide.kind === "diagram" && slide.visual?.labels?.length) {
    return { type: "text" }; // collector supplies strokes; absent = downgrade
  }
  return { type: "text" };
}

export function buildBundle(
  reel: Reel,
  stages: ReadonlyMap<string, BundleStage>,
  ink: ReadonlyMap<string, readonly DrawStroke[]>,
  meta: { exportedBy: string; now?: () => string },
): ReelBundle {
  const slides = normalizeReelToSlides(reel).slides.map((s): BundleSlide => {
    const anchor =
      s.kind === "spotlight" && s.anchor
        ? { sessionId: s.anchor.sessionId, eventId: s.anchor.eventId }
        : undefined;
    const slideInk = ink.get(s.id);
    return {
      id: s.id,
      kind: s.kind,
      role: s.role ?? "body",
      line: s.line,
      caption: captionFor(s),
      ...(anchor ? { anchor } : {}),
      stage: stages.get(s.id) ?? stages.get(anchor?.sessionId ?? "") ?? defaultStage(s),
      ...(slideInk && slideInk.length > 0 ? { ink: slideInk } : {}),
    };
  });
  return {
    v: 1,
    kind: "openstory.reel-bundle",
    exportedAt: meta.now?.() ?? new Date().toISOString(),
    exportedBy: meta.exportedBy,
    reel: {
      id: reel.id,
      title: reel.title,
      author: reel.author,
      created: reel.created,
      slides,
    },
    scan: { v: 1, findings: 0, acknowledged: false },
  };
}

/** Strip tags cheaply for scan purposes only (never for rendering). */
function textContentOf(html: string): string {
  return html.replace(/<[^>]*>/g, " ");
}

export function bundleText(
  bundle: ReelBundle,
): { slideId: string; field: string; text: string }[] {
  const rows: { slideId: string; field: string; text: string }[] = [];
  for (const s of bundle.reel.slides) {
    rows.push({ slideId: s.id, field: "line", text: s.line });
    if (s.stage.type === "snapshot") {
      rows.push({ slideId: s.id, field: "snapshot", text: textContentOf(s.stage.html) });
    }
    if (s.stage.type === "strokes") {
      for (const st of s.stage.strokes) {
        if (st.type === "text") rows.push({ slideId: s.id, field: "stroke-text", text: st.text });
      }
    }
  }
  return rows;
}
```

Note for the implementer: check `Slide`'s actual anchor field shape in `@/lib/reel-slide` (it is `anchor?: SlideAnchor`) and adjust the test fixture's `Reel` cast if `Reel`'s type requires more fields — extend the fixture, never weaken the assertion. The stage lookup by slide id is the contract; the `anchor?.sessionId` fallback lookup is NOT — remove it if the tests pass without it (they should; it is not in the spec).

- [ ] **Step 4: Run to verify GREEN** — same command. Fix type mismatches against the real `Reel`/`Slide` types by adjusting the implementation, not the spec's schema.

- [ ] **Step 5: Full suite + tsc, commit**

```bash
cd ui && npm test && npx tsc --noEmit
git add ui/src/lib/reel-bundle.ts ui/tests/lib/reel-bundle.test.ts
git commit -m "feat(export): ReelBundle schema + builder — the video-ready export contract"
```

---

### Task 2: `export-sanitize.ts` — allowlist DOM sanitizer

**Files:**
- Create: `ui/src/lib/export-sanitize.ts`
- Test: `ui/tests/lib/export-sanitize.test.ts`

**Interfaces:**
- Produces: `sanitizeSnapshotHtml(html: string): string`.

- [ ] **Step 1: Write the failing tests**

```ts
import { describe, expect, it } from "vitest";
import { sanitizeSnapshotHtml } from "@/lib/export-sanitize";

describe("sanitizeSnapshotHtml", () => {
  it("strips script and iframe elements entirely", () => {
    const out = sanitizeSnapshotHtml(
      "<div>ok<script>alert(1)</script><iframe src=\"x\"></iframe></div>",
    );
    expect(out).toContain("ok");
    expect(out).not.toContain("script");
    expect(out).not.toContain("iframe");
  });

  it("strips event handlers and javascript: URLs", () => {
    const out = sanitizeSnapshotHtml(
      "<a href=\"javascript:evil()\" onclick=\"evil()\">x</a>",
    );
    expect(out).not.toContain("onclick");
    expect(out).not.toContain("javascript:");
  });

  it("drops external images but keeps data: images", () => {
    const out = sanitizeSnapshotHtml(
      "<img src=\"https://evil.example/x.png\"><img src=\"data:image/png;base64,AA==\">",
    );
    expect(out).not.toContain("evil.example");
    expect(out).toContain("data:image/png");
  });

  it("keeps class names, inline styles, and text", () => {
    const out = sanitizeSnapshotHtml(
      "<div class=\"card\" style=\"color:red\">payload</div>",
    );
    expect(out).toContain("class=\"card\"");
    expect(out).toContain("color:red");
    expect(out).toContain("payload");
  });

  it("strips url() references to external hosts inside style attributes", () => {
    const out = sanitizeSnapshotHtml(
      "<div style=\"background:url(https://evil.example/x)\">x</div>",
    );
    expect(out).not.toContain("evil.example");
  });

  it("is idempotent", () => {
    const once = sanitizeSnapshotHtml("<div class=\"a\" onclick=\"e()\">t</div>");
    expect(sanitizeSnapshotHtml(once)).toBe(once);
  });
});
```

- [ ] **Step 2: RED** — `npx vitest run tests/lib/export-sanitize.test.ts`.

- [ ] **Step 3: Implement** — DOMParser walk (jsdom provides DOMParser in the vitest environment):

```ts
/** Allowlist sanitizer for spotlight snapshots. What leaves the machine is
 *  exactly what was on screen — inert. Never regex-over-HTML. */

const DROP_ELEMENTS = new Set(["SCRIPT", "IFRAME", "OBJECT", "EMBED", "LINK", "META", "STYLE", "FORM", "INPUT", "BUTTON", "AUDIO", "VIDEO", "SOURCE"]);
const KEEP_ATTRS = new Set(["class", "style", "title", "alt", "colspan", "rowspan", "datetime", "aria-label", "role"]);

function cleanStyle(value: string): string {
  // drop any url(...) that is not a data: URI
  return value.replace(/url\(\s*(['"]?)(?!data:)[^)]*\1\s*\)/gi, "none");
}

export function sanitizeSnapshotHtml(html: string): string {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const walk = (el: Element): void => {
    for (const child of Array.from(el.children)) {
      if (DROP_ELEMENTS.has(child.tagName)) {
        child.remove();
        continue;
      }
      for (const attr of Array.from(child.attributes)) {
        const name = attr.name.toLowerCase();
        if (name === "src") {
          if (!attr.value.trim().toLowerCase().startsWith("data:")) child.removeAttribute(attr.name);
          continue;
        }
        if (name === "href") {
          child.removeAttribute(attr.name); // snapshots are inert — no links out
          continue;
        }
        if (name === "style") {
          child.setAttribute("style", cleanStyle(attr.value));
          continue;
        }
        if (!KEEP_ATTRS.has(name)) child.removeAttribute(attr.name);
      }
      walk(child);
    }
  };
  walk(doc.body);
  return doc.body.innerHTML;
}
```

- [ ] **Step 4: GREEN**, adjust only the implementation. **Step 5: full suite + tsc, commit** `feat(export): allowlist snapshot sanitizer`.

---

### Task 3: `export-scan.ts` — sensitive-content scanner

**Files:**
- Create: `ui/src/lib/export-scan.ts`
- Test: `ui/tests/lib/export-scan.test.ts`

**Interfaces:**
- Consumes: `bundleText`, `ReelBundle` (Task 1).
- Produces: `Finding { slideId: string; family: string; excerpt: string }`; `scanBundle(bundle: ReelBundle): Finding[]`.

- [ ] **Step 1: Failing tests** — one positive + one negative per family:

```ts
import { describe, expect, it } from "vitest";
import { scanBundle } from "@/lib/export-scan";
import { buildBundle } from "@/lib/reel-bundle";
import type { Reel } from "@/lib/reels-api";

function reelWithLine(line: string): Reel {
  return { id: "r", title: "t", author: "a", created: "c", stops: [{ line, kind: "title" }] } as Reel;
}
const scanOf = (line: string) =>
  scanBundle(buildBundle(reelWithLine(line), new Map(), new Map(), { exportedBy: "x" }));

describe("scanBundle families", () => {
  it("flags AWS access key ids", () => {
    expect(scanOf("key AKIAIOSFODNN7EXAMPLE here")[0]?.family).toBe("aws-key");
  });
  it("flags secret-assignment shapes", () => {
    expect(scanOf("api_key = \"sk-live-abcdef1234567890\"").length).toBeGreaterThan(0);
  });
  it("flags PEM blocks", () => {
    expect(scanOf("-----BEGIN RSA PRIVATE KEY-----")[0]?.family).toBe("pem");
  });
  it("flags bearer tokens", () => {
    expect(scanOf("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.x.y").length).toBeGreaterThan(0);
  });
  it("flags email addresses", () => {
    expect(scanOf("mail me at someone@example.com")[0]?.family).toBe("email");
  });
  it("flags absolute home paths", () => {
    expect(scanOf("read /Users/somebody/secrets.txt")[0]?.family).toBe("home-path");
  });
  it("stays quiet on ordinary prose and trims excerpts to 80 chars", () => {
    expect(scanOf("We fixed the caption bar and merged the PR.")).toEqual([]);
    const f = scanOf("x".repeat(200) + " AKIAIOSFODNN7EXAMPLE");
    expect(f[0]!.excerpt.length).toBeLessThanOrEqual(80);
  });
});
```

- [ ] **Step 2: RED.** **Step 3: Implement:**

```ts
import { bundleText, type ReelBundle } from "@/lib/reel-bundle";

export interface Finding {
  readonly slideId: string;
  readonly family: string;
  readonly excerpt: string;
}

const FAMILIES: readonly { family: string; re: RegExp }[] = [
  { family: "aws-key", re: /\bAKIA[0-9A-Z]{16}\b/ },
  { family: "secret-assign", re: /\b(api[_-]?key|token|secret|password|passwd)\b\s*[:=]\s*["']?[^\s"']{8,}/i },
  { family: "pem", re: /-----BEGIN [A-Z ]*PRIVATE KEY-----/ },
  { family: "bearer", re: /\bBearer\s+[A-Za-z0-9\-_.=]{20,}/ },
  { family: "email", re: /\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/ },
  { family: "home-path", re: /\/(Users|home)\/[A-Za-z0-9._-]+\// },
];

export function scanBundle(bundle: ReelBundle): Finding[] {
  const out: Finding[] = [];
  for (const row of bundleText(bundle)) {
    for (const { family, re } of FAMILIES) {
      const m = re.exec(row.text);
      if (!m) continue;
      const at = Math.max(0, m.index - 20);
      out.push({ slideId: row.slideId, family, excerpt: row.text.slice(at, at + 80).trim() });
    }
  }
  return out;
}
```

- [ ] **Step 4: GREEN.** If the `secret-assign` positive also matches another family, assert on `.some(f => f.family === ...)` instead of `[0]` — assertion precision over order assumptions. **Step 5: suite + tsc, commit** `feat(export): bundle sensitive-content scanner`.

---

### Task 4: `export-template.ts` — `bakeReelHtml`

**Files:**
- Create: `ui/src/lib/export-template.ts`
- Test: `ui/tests/lib/export-template.test.ts`

**Interfaces:**
- Consumes: `ReelBundle` (Task 1), `pathToSvgD` semantics (reimplemented inline in the template's JS — the template cannot import).
- Produces: `bakeReelHtml(bundle: ReelBundle): string` — a complete `<!doctype html>` document.

- [ ] **Step 1: Failing tests**

```ts
import { describe, expect, it } from "vitest";
import { bakeReelHtml } from "@/lib/export-template";
import { buildBundle } from "@/lib/reel-bundle";
import type { Reel } from "@/lib/reels-api";

const REEL = {
  id: "r1", title: "My reel", author: "max", created: "c",
  opener: "Bottom line.",
  stops: [
    { line: "Spot.", kind: "spotlight", sessionId: "s1", eventId: "e1" },
    { line: "Diagram.", kind: "diagram" },
  ],
} as Reel;
const bundle = buildBundle(
  REEL,
  new Map([["s1", { type: "snapshot", html: "<div class=\"snap\">CONTENT</div>" }]]),
  new Map(),
  { exportedBy: "max", now: () => "t0" },
);

describe("bakeReelHtml", () => {
  const doc = bakeReelHtml(bundle);

  it("embeds exactly one parseable bundle JSON equal to its input", () => {
    const m = doc.match(/<script type="application\/json" id="reel-bundle">([\s\S]*?)<\/script>/g);
    expect(m?.length).toBe(1);
    const inner = m![0]!.replace(/^<script[^>]*>/, "").replace(/<\/script>$/, "");
    expect(JSON.parse(inner)).toEqual(bundle);
  });

  it("makes no external requests: no http(s) src/href, no external script/link", () => {
    expect(doc).not.toMatch(/(src|href)=["']https?:/i);
    expect(doc).not.toContain("<link rel=\"stylesheet\" href");
  });

  it("renders one section per slide and suppresses caption on title slides", () => {
    expect((doc.match(/data-slide=/g) ?? []).length).toBe(bundle.reel.slides.length);
    // opener is a title card: its section carries data-caption="" (empty)
    expect(doc).toMatch(/data-slide="s0"[^>]*data-caption=""/);
  });

  it("escapes </script> inside the embedded JSON", () => {
    const evil = buildBundle(
      { ...REEL, stops: [{ line: "x</script><script>alert(1)", kind: "title" }] } as Reel,
      new Map(), new Map(), { exportedBy: "max" },
    );
    const d = bakeReelHtml(evil);
    // the raw close tag must never appear inside the JSON block
    const inner = d.split("id=\"reel-bundle\">")[1]!.split("</script>")[0]!;
    expect(inner).not.toContain("</script>");
    expect(JSON.parse(inner.replace(/<\\\/script>/g, "</script>")).reel.slides.length).toBe(1);
  });
});
```

- [ ] **Step 2: RED.** **Step 3: Implement.** Core requirements for the document (the implementer writes the full template; these are binding):

```ts
export function bakeReelHtml(bundle: ReelBundle): string {
  const json = JSON.stringify(bundle).replace(/<\/script>/g, "<\\/script>");
  // …template literal document…
}
```

- One `<section data-slide="<id>" data-caption="<caption ?? ''>">` per slide, hidden except the active one.
- Stage rendering in ~100 lines of inline vanilla JS reading the embedded JSON at load: `text` → the line centered large; `strokes` → SVG `viewBox="0 0 1000 1000"` scaling unit coords ×1000 (paths, lines, circles, ellipses, text — same shapes as `DrawStroke`); `image` → `<img src="<dataUri>">`; `snapshot` → `innerHTML` of the stored fragment inside a stage frame. Ink renders as a second SVG overlay per slide.
- Controls: Play/Pause button, ‹ ›, progress dots, click-to-advance, arrow keys; caption bar shows `caption` only when non-empty; caption-paced autoplay (`max(3500, words/3*1000+2000)` ms — same formula as the live player's no-speech fallback); 🔊 toggle using `speechSynthesis` when available, speaking `line` per slide.
- Inline CSS only: dark theme (`#1a1b26` bg, `#cdd6f9` text, `#7aa2f7` accent), plus a `.snap`-scoped style subset for snapshot fragments (spacing, monospace for `pre/code`, muted borders). System font stack; no webfonts.
- Footer line: `Exported from OpenStory · <exportedAt> · by <exportedBy> · scan: <clean|N findings acknowledged>`.

- [ ] **Step 4: GREEN.** **Step 5: suite + tsc, commit** `feat(export): bakeReelHtml — self-contained offline reel document`.

---

### Task 5: collector + dialog + affordances

**Files:**
- Create: `ui/src/lib/export-collect.ts`
- Create: `ui/src/components/reels/ExportReelDialog.tsx`
- Modify: `ui/src/components/reels/ReelsView.tsx`
- Test: `ui/tests/components/ExportReelDialog.test.tsx`

**Interfaces:**
- Consumes: everything above; `fetchReel` from `@/lib/reels-api`; `loadBeatInkStore`, `getBeatInk` from `@/lib/reel-annotate`; `normalizeReelToSlides` for slide↔beat index mapping; `diagramLabelsToStrokes` from `@/lib/reel-visual` for diagram stages.
- Produces: `collectBundle(reelId: string, opts?: { exportedBy?: string }): Promise<{ bundle: ReelBundle; degraded: string[] }>` (degraded = slide ids that fell back to text stages); `ExportReelDialog({ reelId, onClose })`.

- [ ] **Step 1: Failing component test** (jsdom; mock `fetch` the way `ui/tests/components/reels-view.test.tsx` stubs the reels API — read that file's `stubReelsFetch` helper first and follow its pattern):

```tsx
// ui/tests/components/ExportReelDialog.test.tsx — assertions that matter:
// 1. renders scan-clean state: primary button text "Save reel file"
// 2. with a finding planted (a stop line containing "AKIAIOSFODNN7EXAMPLE"):
//    button text becomes "Export anyway" and the finding's family + slide id render
// 3. clicking "Export anyway" produces a download whose object URL blob, read back,
//    contains "\"acknowledged\":true"  (capture URL.createObjectURL with vi.spyOn)
// 4. the preview iframe's srcdoc contains the embedded bundle JSON marker id="reel-bundle"
```

Write all four as real `it()` blocks with real assertions before implementing.

- [ ] **Step 2: RED.** **Step 3: Implement `export-collect.ts`:**

- `fetchReel(reelId)`; map slides via `normalizeReelToSlides`.
- Spotlight stages: render with the real spotlight renderer into a detached container. Read `EventSpotlight`'s props first; if it fetches its own data, mount it with `createRoot` in a hidden div, wait one macrotask + a `requestAnimationFrame` (or poll ≤2s for non-empty innerHTML), capture, `sanitizeSnapshotHtml`, unmount. On timeout: degraded text stage + slide id in `degraded`.
- Diagram stages: `diagramLabelsToStrokes(labels, { title })` — same call `ReelBeatStage` makes; for journey-fetch diagrams reuse its fetch shape (`/api/sessions/{sid}/tool-journey`), degrading on failure.
- Image stages: fetch → blob → FileReader data URI; degrade on failure.
- Ink: `loadBeatInkStore()` + `getBeatInk(store, { reelId, beatIndex })` mapped per body slide (beat index = body position, matching `BeatInkLayer`'s indexing — verify against `playerToSlideIndex` before assuming).
- `exportedBy`: `opts.exportedBy ?? "openstory"` — v1 does not fetch person config (YAGNI; the field is plumbed).

**Implement `ExportReelDialog.tsx`:** modal over the reels view; on mount `collectBundle` → `scanBundle` → `bakeReelHtml`; sandboxed `<iframe srcdoc={html} sandbox="allow-scripts">` preview; degraded-slide notices; findings list grouped by slide with family labels; buttons per spec (clean → "Save reel file"; findings → "Export anyway" which re-bakes with `scan: { v:1, findings: n, acknowledged: true }` + "Cancel"). Download: `new Blob([html], { type: "text/html" })` → `URL.createObjectURL` → `<a download="${slug(title)}.reel.html">` click → revoke.

**Wire affordances in `ReelsView.tsx`:** an "Export" button on each reel row (`data-testid="reel-export-<id>"`, stopPropagation like the ▶ Play button) and on the player idle screen; both set `exportingReelId` state rendering the dialog.

- [ ] **Step 4: GREEN** — component test passes; full suite; tsc.
- [ ] **Step 5: Commit** `feat(export): reel export dialog — collect, preview, scan, save`.

---

### Task 6: docs + final verification

**Files:**
- Modify: `docs/BACKLOG.md`, `docs/agent-in-ui.md`

- [ ] **Step 1: BACKLOG entry** (under the reels theme):

```markdown
- **Video export — second ReelBundle consumer.** The HTML export bakes a
  versioned ReelBundle (schema: ui/src/lib/reel-bundle.ts; spec:
  docs/superpowers/specs/2026-08-22-reel-export-design.md). The video renderer
  from docs/research/reel-to-video.md should consume the same bundle: headless
  render of each slide stage → frames, TTS for lines, mux; embed the bundle as
  MP4 metadata + per-beat chapter provenance (platforms that transcode strip
  metadata — on-screen citations stay burned in).
```

- [ ] **Step 2: `docs/agent-in-ui.md`** — one line in the appropriate section: reel export is a human affordance in v1 (no control verb); agents can read reels via MCP but export decisions (preview + scan acknowledgment) belong to the human.

- [ ] **Step 3: Full gate** — `cd ui && npm test && npx tsc --noEmit` and `python3 scripts/check_docs.py` (from repo root). All green, exact counts in the report.

- [ ] **Step 4: Commit** `docs(export): backlog video consumer + agent-in-ui export note`.

---

## Verification (whole plan)

- [ ] `cd ui && npm test` — full suite green (report exact counts; `tests/lib/timeline-bench.test.ts` is a known load-flaky perf bench — only-failure means note-and-proceed).
- [ ] `npx tsc --noEmit` clean; `python3 scripts/check_docs.py` green.
- [ ] Manual walkthrough (controller, Chrome-driven, after all tasks): export a real reel with a spotlight + diagram + ink; open the downloaded file from disk; verify slides advance, captions suppress on titles, ink renders, 🔊 speaks, and devtools network tab shows zero requests. Plant a fake `AKIA…` line in a scratch reel and verify the gate flips to "Export anyway".

## Out of scope (from the spec — do not build)

Video renderer; hosted links; re-import; redaction-in-place; agent-triggered export.
