/**
 * Single slide format for reels — normalize legacy opener/stops/closer
 * into one ordered list. Tools, stage, and ink all key off Slide.
 */

import type { Reel, ReelStop, ReelVisual } from "@/lib/reels-api";
import { normalizeStopKind, type ReelStopKind } from "@/lib/reel-visual";

export type SlideKind = ReelStopKind;

export interface SlideAnchor {
  readonly sessionId: string;
  readonly eventId: string;
  readonly clipAt?: string;
}

export interface SlideVisual {
  readonly title?: string;
  readonly labels?: readonly string[];
  readonly imageHref?: string;
  readonly sessionId?: string;
}

/** One narrative unit — the only thing the player/tools should think about. */
export interface Slide {
  readonly id: string;
  readonly index: number;
  readonly kind: SlideKind;
  readonly line: string;
  readonly anchor?: SlideAnchor;
  readonly visual?: SlideVisual;
  /** true if this slide came from legacy opener/closer (not stops[]). */
  readonly role?: "opener" | "body" | "closer";
}

export interface SlideReel {
  readonly id: string;
  readonly title: string;
  readonly author: string;
  readonly slides: readonly Slide[];
}

function visualFromStop(v: ReelVisual | undefined): SlideVisual | undefined {
  if (!v) return undefined;
  const out: SlideVisual = {
    ...(v.title ? { title: v.title } : {}),
    ...(v.labels?.length ? { labels: v.labels } : {}),
    ...(v.imageHref ? { imageHref: v.imageHref } : {}),
    ...(v.sessionId ? { sessionId: v.sessionId } : {}),
  };
  return Object.keys(out).length ? out : undefined;
}

function stopToSlide(stop: ReelStop, index: number, id: string): Slide {
  const kind = normalizeStopKind(stop.kind);
  const hasAnchor =
    kind === "spotlight" &&
    typeof stop.sessionId === "string" &&
    stop.sessionId.length > 0 &&
    typeof stop.eventId === "string" &&
    stop.eventId.length > 0;
  return {
    id,
    index,
    kind: hasAnchor || kind === "spotlight" ? (hasAnchor ? "spotlight" : kind) : kind,
    line: stop.line,
    ...(hasAnchor
      ? {
          anchor: {
            sessionId: stop.sessionId!,
            eventId: stop.eventId!,
            ...(stop.clipAt ? { clipAt: stop.clipAt } : {}),
          },
        }
      : {}),
    ...(visualFromStop(stop.visual) ? { visual: visualFromStop(stop.visual) } : {}),
    role: "body",
  };
}

/**
 * Pure: legacy Reel → ordered slides (opener + body + closer).
 * Ink keys use `slide.index` in this list (0..n-1).
 */
export function normalizeReelToSlides(reel: Reel): SlideReel {
  const slides: Slide[] = [];
  let i = 0;
  if (reel.opener?.trim()) {
    slides.push({
      id: `${reel.id}:opener`,
      index: i++,
      kind: "title",
      line: reel.opener.trim(),
      role: "opener",
    });
  }
  for (let s = 0; s < reel.stops.length; s++) {
    slides.push(stopToSlide(reel.stops[s]!, i, `${reel.id}:s${s}`));
    i++;
  }
  if (reel.closer?.trim()) {
    slides.push({
      id: `${reel.id}:closer`,
      index: i++,
      kind: "title",
      line: reel.closer.trim(),
      role: "closer",
    });
  }
  // Re-index in case of sparse
  const fixed = slides.map((sl, idx) => ({ ...sl, index: idx }));
  return {
    id: reel.id,
    title: reel.title,
    author: reel.author,
    slides: fixed,
  };
}

/** Map player phase+stopIndex → unified slide index. */
export function playerToSlideIndex(
  slides: readonly Slide[],
  phase: "opener" | "stop" | "closer" | "idle" | "done",
  stopIndex: number,
): number | null {
  if (phase === "idle" || phase === "done") return null;
  if (phase === "opener") {
    const o = slides.findIndex((s) => s.role === "opener");
    return o >= 0 ? o : 0;
  }
  if (phase === "closer") {
    const c = slides.findIndex((s) => s.role === "closer");
    return c >= 0 ? c : slides.length - 1;
  }
  // body stop index → slide index (prefer role body; else non-bookend slides)
  const bodySlides = slides.filter((s) => s.role === "body");
  const list = bodySlides.length
    ? bodySlides
    : slides.filter((s) => s.role !== "opener" && s.role !== "closer");
  const hit = list[stopIndex];
  return hit ? hit.index : null;
}

export function slideInkKey(reelId: string, slideIndex: number): string {
  return `${reelId}:${slideIndex}`;
}

/** Caption line for the player's bottom bar — null when the stage already
 *  renders the line full-screen (title cards), so it is never shown twice. */
export function captionFor(slide: Pick<Slide, "kind" | "line">): string | null {
  return slide.kind === "title" ? null : slide.line;
}
