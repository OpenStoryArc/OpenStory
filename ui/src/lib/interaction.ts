/** The READ half of the agent-in-UI seam, client side: the UI reports the
 *  human's interactions so they become first-class events in OpenStory (a
 *  "viewing session"), projectable to `ui_state` and replayable. This module is
 *  the pure mapping route → interaction payload; the POST is the side effect. */

import type { HashRoute } from "@/lib/hash-route";

export type InteractionKind = "navigate" | "filter" | "select" | "zoom" | "view";

export interface InteractionPayload {
  readonly kind: InteractionKind;
  readonly view: string;
  readonly session_id?: string;
  readonly detailView?: string;
  readonly eventId?: string;
  readonly filters?: unknown;
}

/** A navigation interaction from the current route. Coarse but enough to
 *  reconstruct "where you were" and replay the journey (finer kinds — filter/
 *  select/zoom — are emitted by the individual views as they wire in). */
export function interactionFromRoute(route: HashRoute): InteractionPayload {
  const p: {
    kind: InteractionKind; view: string; session_id?: string;
    detailView?: string; eventId?: string; filters?: unknown;
  } = { kind: "navigate", view: route.view };
  if (route.sessionId) p.session_id = route.sessionId;
  if (route.detailView) p.detailView = route.detailView;
  if (route.eventId) p.eventId = route.eventId;
  if (route.overview?.filters && Object.keys(route.overview.filters).length > 0) {
    p.filters = route.overview.filters;
  }
  return p;
}

/** Fire-and-forget POST of an interaction. Never throws (best-effort telemetry
 *  of your own use — must never break the UI). */
export function postInteraction(payload: InteractionPayload, baseUrl = ""): void {
  void fetch(`${baseUrl}/api/interactions`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  }).catch(() => {});
}
