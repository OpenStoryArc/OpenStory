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
      stage: stages.get(s.id) ?? defaultStage(s),
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
