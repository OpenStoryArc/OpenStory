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
