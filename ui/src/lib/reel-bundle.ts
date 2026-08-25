/**
 * ReelBundle — the export contract. The HTML artifact is its first renderer,
 * the video pipeline (docs/research/reel-to-video.md) its second. Pure: the
 * collector supplies stages/ink; this module only assembles and flattens.
 */

import type { Reel } from "@/lib/reels-api";
import { captionFor, normalizeReelToSlides, type Slide } from "@/lib/reel-slide";
import type { DrawStroke } from "@/lib/draw";
import { cleanUrl } from "@/lib/export-sanitize";

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

/** Only `data:` image strokes are inert in a self-contained export — anything
 *  else (http(s):, protocol-relative, javascript:, ...) is a live network
 *  reference that would fire the moment the baked file (or its preview) is
 *  opened. `draw.ts::normalizeStroke` allows http(s) at capture time (ink is
 *  drawn live, against a real screen), so the export boundary is where it
 *  must be closed off — not the capture boundary. */
function cleanStrokeImageHref(href: string): string {
  return href.trim().toLowerCase().startsWith("data:") ? href : "";
}

/** Make one stroke inert for a self-contained export: gate `image` hrefs to
 *  `data:` only, and scrub any funcIRI-capable presentation attribute
 *  (`fill`, `stroke`) of external `url(...)` references, reusing the exact
 *  rule `export-sanitize.ts` applies to snapshot HTML (`cleanUrl`) so the two
 *  scrub paths — snapshot markup and stroke geometry — can't drift apart. */
export function sanitizeStroke(s: DrawStroke): DrawStroke {
  switch (s.type) {
    case "image":
      return { ...s, href: cleanStrokeImageHref(s.href) };
    case "path":
    case "circle":
    case "ellipse":
      return {
        ...s,
        fill: s.fill === undefined ? undefined : cleanUrl(s.fill),
        stroke: s.stroke === undefined ? undefined : cleanUrl(s.stroke),
      };
    case "line":
      return { ...s, stroke: s.stroke === undefined ? undefined : cleanUrl(s.stroke) };
    case "text":
      return { ...s, fill: s.fill === undefined ? undefined : cleanUrl(s.fill) };
    default:
      return s;
  }
}

function sanitizeStrokes(strokes: readonly DrawStroke[]): DrawStroke[] {
  return strokes.map(sanitizeStroke);
}

/** Make a stage inert in place — only `strokes` stages carry stroke geometry
 *  (image/snapshot stages are handled by the collector / export-sanitize). */
function sanitizeStage(stage: BundleStage): BundleStage {
  if (stage.type === "strokes") {
    return { type: "strokes", strokes: sanitizeStrokes(stage.strokes) };
  }
  return stage;
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
      stage: sanitizeStage(stages.get(s.id) ?? defaultStage(s)),
      ...(slideInk && slideInk.length > 0 ? { ink: sanitizeStrokes(slideInk) } : {}),
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
    if (s.ink) {
      for (const st of s.ink) {
        if (st.type === "text") rows.push({ slideId: s.id, field: "stroke-text", text: st.text });
      }
    }
  }
  return rows;
}
