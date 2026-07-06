/** The READ half of the agent-in-UI seam, client side: the human's interactions
 *  become first-class events in OpenStory (a "viewing session"), projectable to
 *  `ui_state` and replayable.
 *
 *  User interactions are TYPED — a schema per kind, exactly parallel to how
 *  agent activity is typed (CloudEvents, turn.sentence patterns, eval-apply).
 *  The discriminated union below IS that schema; the `kind` is the subtype the
 *  server records (`interaction.<kind>`). Agent patterns and user patterns are
 *  the same class of citizen on the same bus. */

import type { HashRoute } from "@/lib/hash-route";

export type InteractionKind = "navigate" | "filter" | "select" | "zoom" | "view";

/** Typed interaction schema — one shape per kind. Grow a variant's fields for
 *  higher replay fidelity without touching the others. */
export type Interaction =
  | { kind: "navigate"; view: string; session_id?: string; detailView?: string; eventId?: string; filters?: unknown }
  | { kind: "filter"; view: string; filters: unknown; session_id?: string }
  | { kind: "select"; view: string; session_id: string; turn?: number; eventId?: string; eval?: string }
  | { kind: "zoom"; view: string; mode?: string; zoom?: number }
  | { kind: "view"; view: string };

/** A navigation interaction from the current route. Coarse but enough to
 *  reconstruct "where you were" and replay the journey (finer kinds — filter/
 *  select/zoom — are emitted by the individual views). */
export function interactionFromRoute(route: HashRoute): Extract<Interaction, { kind: "navigate" }> {
  const p: Extract<Interaction, { kind: "navigate" }> = { kind: "navigate", view: route.view };
  if (route.sessionId) p.session_id = route.sessionId;
  if (route.detailView) p.detailView = route.detailView;
  if (route.eventId) p.eventId = route.eventId;
  if (route.explore?.filters && Object.keys(route.explore.filters).length > 0) {
    p.filters = route.explore.filters;
  }
  return p;
}

/** A "select" interaction — the user clicked INTO a thing (a session, an event,
 *  a wedge). eventId is omitted when the whole session is the target. Pure so
 *  the capture shape is testable + the same builder feeds replay. */
export function selectInteraction(view: string, sessionId: string, eventId?: string): Extract<Interaction, { kind: "select" }> {
  const i: Extract<Interaction, { kind: "select" }> = { kind: "select", view, session_id: sessionId };
  if (eventId) i.eventId = eventId;
  return i;
}

/** A "filter" interaction — the user narrowed a view (facets, search). */
export function filterInteraction(view: string, filters: unknown, sessionId?: string): Extract<Interaction, { kind: "filter" }> {
  const i: Extract<Interaction, { kind: "filter" }> = { kind: "filter", view, filters };
  if (sessionId) i.session_id = sessionId;
  return i;
}

/** A "zoom" interaction — the user changed a view's zoom/mode (drill, tempo). */
export function zoomInteraction(view: string, opts?: { mode?: string; zoom?: number }): Extract<Interaction, { kind: "zoom" }> {
  const i: Extract<Interaction, { kind: "zoom" }> = { kind: "zoom", view };
  if (opts?.mode) i.mode = opts.mode;
  if (opts?.zoom != null) i.zoom = opts.zoom;
  return i;
}

/** Fire-and-forget POST of a typed interaction. Never throws (best-effort
 *  telemetry of your own use — must never break the UI). */
export function postInteraction(i: Interaction, baseUrl = ""): void {
  void fetch(`${baseUrl}/api/interactions`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(i),
  }).catch(() => {});
}
