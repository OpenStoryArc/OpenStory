/**
 * export-collect.ts — the one side-effectful module in the export pipeline
 * (see docs/superpowers/specs/2026-08-22-reel-export-design.md §Components).
 * Gathers everything `buildBundle` needs from the running app:
 *   - fetch the reel, normalize to slides (Task 4's `normalizeReelToSlides`)
 *   - render each spotlight stop offscreen via the REAL `EventSpotlight` and
 *     capture its sanitized innerHTML (what leaves the machine is exactly
 *     what was on screen — never raw event payloads)
 *   - resolve diagram strokes from labels or a tool-journey fetch, mirroring
 *     `ReelBeatStage`'s own fetch shape so the export matches the live player
 *   - inline images to data URIs
 *   - read beat ink from localStorage (`reel-annotate`), keyed by the SAME
 *     beatIndex space `BeatInkLayer` writes to (verified against
 *     `playerToSlideIndex`/`ReelsView` below — see the ink section)
 *
 * Never mutates: read-only against history (spotlight fetch), read-only
 * against ink (localStorage read only). No new CloudEvents.
 */

import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { EventSpotlight } from "@/components/control/EventSpotlight";
import type { DrawStroke } from "@/lib/draw";
import { sanitizeSnapshotHtml } from "@/lib/export-sanitize";
import { getBeatInk, loadBeatInkStore } from "@/lib/reel-annotate";
import { buildBundle, type BundleStage, type ReelBundle } from "@/lib/reel-bundle";
import { normalizeReelToSlides, type Slide } from "@/lib/reel-slide";
import { fetchReel, type Reel, type ReelStop } from "@/lib/reels-api";
import { diagramLabelsToStrokes } from "@/lib/reel-visual";

const SPOTLIGHT_CAPTURE_TIMEOUT_MS = 2000;
const SPOTLIGHT_POLL_INTERVAL_MS = 50;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Mount the real `EventSpotlight` offscreen (detached from layout, never
 * painted), wait until it settles out of its "loading" state (ready OR
 * missing both render real, honest content — a missing event renders the
 * player's own "wasn't found" copy, never a fabrication), capture + sanitize
 * its innerHTML, then unmount. Returns null on timeout — the caller degrades
 * that slide to a plain text stage.
 */
async function captureSpotlightSnapshot(
  sessionId: string,
  eventId: string,
  clipAt: string | undefined,
): Promise<string | null> {
  const host = document.createElement("div");
  host.style.position = "fixed";
  host.style.left = "-99999px";
  host.style.top = "0";
  host.style.width = "1024px";
  host.style.height = "768px";
  host.style.pointerEvents = "none";
  document.body.appendChild(host);

  const root = createRoot(host);
  try {
    root.render(
      createElement(EventSpotlight, {
        sessionId,
        eventId,
        clipAt,
        onClose: () => {},
      }),
    );

    const deadline = Date.now() + SPOTLIGHT_CAPTURE_TIMEOUT_MS;
    let card: Element | null = null;
    while (Date.now() < deadline) {
      const candidate = host.querySelector(".spotlight-card");
      if (candidate && candidate.innerHTML.trim().length > 0) {
        card = candidate;
        break;
      }
      await sleep(SPOTLIGHT_POLL_INTERVAL_MS);
    }
    if (!card) return null;
    return sanitizeSnapshotHtml(card.innerHTML);
  } finally {
    root.unmount();
    host.remove();
  }
}

async function imageToDataUri(href: string): Promise<string | null> {
  try {
    const res = await fetch(href);
    if (!res.ok) return null;
    const blob = await res.blob();
    return await new Promise<string | null>((resolve) => {
      const reader = new FileReader();
      reader.onerror = () => resolve(null);
      reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : null);
      reader.readAsDataURL(blob);
    });
  } catch {
    return null;
  }
}

/** Same shape as `ReelBeatStage`'s own tool-journey fetch + consecutive-tool
 *  collapse, so an exported diagram matches what the live player would have
 *  shown. Returns null on fetch failure (network error or non-ok response) —
 *  the caller degrades. An empty-but-successful journey is NOT a failure
 *  (honest empty state, same as the live player). */
async function fetchJourneyLabels(sessionId: string): Promise<string[] | null> {
  try {
    const res = await fetch(`/api/sessions/${encodeURIComponent(sessionId)}/tool-journey`);
    if (!res.ok) return null;
    const rows: unknown = await res.json();
    const list = Array.isArray(rows) ? rows : [];
    const tools = list
      .map((e) =>
        e && typeof e === "object" && "tool" in e
          ? String((e as { tool?: string }).tool ?? "")
          : "",
      )
      .filter(Boolean);
    const collapsed: string[] = [];
    for (const t of tools) {
      const prev = collapsed[collapsed.length - 1];
      if (prev && prev.startsWith(t)) {
        const m = prev.match(/×(\d+)$/);
        const n = m ? Number(m[1]) + 1 : 2;
        collapsed[collapsed.length - 1] = `${t} ×${n}`;
      } else if (prev === t) {
        collapsed[collapsed.length - 1] = `${t} ×2`;
      } else {
        collapsed.push(t);
      }
    }
    return collapsed.slice(0, 8);
  } catch {
    return null;
  }
}

/** Body slides carry `visual.sessionId` when the reel author set it, but
 *  `Slide` (Task 4's normalized shape) doesn't preserve `ReelStop.sessionId`
 *  as a fallback the way `ReelBeatStage` does — so we reach back into the
 *  raw reel to mirror that fallback exactly. */
function rawStopFor(reel: Reel, slideId: string): ReelStop | undefined {
  const m = /:s(\d+)$/.exec(slideId);
  if (!m) return undefined;
  return reel.stops[Number(m[1])];
}

async function collectStage(
  reel: Reel,
  slide: Slide,
  degraded: string[],
): Promise<BundleStage | null> {
  if (slide.kind === "spotlight" && slide.anchor) {
    const html = await captureSpotlightSnapshot(
      slide.anchor.sessionId,
      slide.anchor.eventId,
      slide.anchor.clipAt,
    );
    if (html == null) {
      degraded.push(slide.id);
      return null;
    }
    return { type: "snapshot", html };
  }

  if (slide.kind === "diagram") {
    const labels = slide.visual?.labels;
    if (labels && labels.length > 0) {
      return {
        type: "strokes",
        strokes: diagramLabelsToStrokes(labels, { title: slide.visual?.title ?? "Diagram" }),
      };
    }
    const sid = slide.visual?.sessionId ?? rawStopFor(reel, slide.id)?.sessionId;
    if (!sid) {
      return {
        type: "strokes",
        strokes: diagramLabelsToStrokes([], { title: slide.visual?.title ?? "Diagram" }),
      };
    }
    const collapsed = await fetchJourneyLabels(sid);
    if (collapsed == null) {
      degraded.push(slide.id);
      return null;
    }
    return {
      type: "strokes",
      strokes: diagramLabelsToStrokes(collapsed, {
        title: slide.visual?.title ?? "Tool journey",
      }),
    };
  }

  if (slide.kind === "image" && slide.visual?.imageHref) {
    const dataUri = await imageToDataUri(slide.visual.imageHref);
    if (!dataUri) {
      degraded.push(slide.id);
      return null;
    }
    return { type: "image", dataUri };
  }

  // title slides (and any malformed spotlight/image/diagram slide missing
  // what it needs) — buildBundle's default text stage already covers this;
  // nothing to collect, nothing to mark degraded.
  return null;
}

export async function collectBundle(
  reelId: string,
  opts?: { readonly exportedBy?: string },
): Promise<{ bundle: ReelBundle; degraded: string[] }> {
  const reel = await fetchReel(reelId);
  if (!reel) throw new Error(`collectBundle: reel "${reelId}" not found`);

  const { slides } = normalizeReelToSlides(reel);
  const degraded: string[] = [];
  const stages = new Map<string, BundleStage>();
  for (const slide of slides) {
    const stage = await collectStage(reel, slide, degraded);
    if (stage) stages.set(slide.id, stage);
  }

  // Ink keying: `BeatInkLayer` (ui/src/components/reels/BeatInkLayer.tsx,
  // mounted from ReelsView's `slideChrome`) is given `beatIndex={slideIndex}`
  // where `slideIndex` comes straight out of `playerToSlideIndex(...)` — the
  // UNIFIED slide-list position (opener included, 0-based), which is exactly
  // `Slide.index` after `normalizeReelToSlides`'s re-index pass. It is NOT
  // scoped to body-only position. So ink for slide `s` lives under
  // `beatIndex: s.index`, not under a body-stop-only counter.
  const inkStore = loadBeatInkStore();
  const ink = new Map<string, readonly DrawStroke[]>();
  for (const slide of slides) {
    const beatInk = getBeatInk(inkStore, { reelId, beatIndex: slide.index });
    if (beatInk.strokes.length > 0) ink.set(slide.id, beatInk.strokes);
  }

  const bundle = buildBundle(reel, stages, ink, {
    exportedBy: opts?.exportedBy ?? "openstory",
  });
  return { bundle, degraded };
}
