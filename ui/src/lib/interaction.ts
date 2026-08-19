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
import type { LayoutEyes } from "@/lib/layout-eyes";
import type { PenSceneWire } from "@/lib/pen-eyes";

export type InteractionKind = "navigate" | "filter" | "select" | "zoom" | "view";

/** Layout eyes payload on the wire (viewport 0..1 targets). */
export type InteractionLayout = ReturnType<
  typeof import("@/lib/layout-eyes").layoutEyesToWire
>;

/** Optional glass geometry + pen — ride free on any interaction body. */
type WithLayout = {
  /** Where attention *is on the glass* (layout eyes). */
  layout?: InteractionLayout | LayoutEyes;
  /** What is on the agent pen (pen eyes) — ui.* ink snapshot. */
  pen?: PenSceneWire;
  /** Glass freehand annotate mode (reels/story overlay). */
  annotate?: boolean;
  /** Active reel when view is reels. */
  reelId?: string;
  /** Active beat index while playing a reel. */
  beatIndex?: number;
  /** Beat-scoped ink snapshot (1:1 with beatIndex). */
  beatInk?: {
    reelId: string;
    beatIndex: number;
    stroke_count: number;
    empty: boolean;
    interactive?: boolean;
    kinds?: Record<string, number>;
  };
  /** Glass-ink snapshot for the current routeGlassKey context (1:1 with the
   *  view/session the ink is drawn on — not the Draw tab's board). */
  glassInk?: {
    key: string;
    stroke_count: number;
  };
};

/** Typed interaction schema — one shape per kind. Grow a variant's fields for
 *  higher replay fidelity without touching the others. */
export type Interaction =
  | ({
      kind: "navigate";
      view: string;
      session_id?: string;
      detailView?: string;
      eventId?: string;
      filePath?: string;
      searchQuery?: string;
      userFilter?: string;
      timeFilter?: string;
      filters?: unknown;
      /** Ephemeral presentation state (spotlight/title) when the client reports it. */
      spotlight?: boolean;
      present_message?: string;
    } & WithLayout)
  | ({ kind: "filter"; view: string; filters: unknown; session_id?: string } & WithLayout)
  | ({
      kind: "select";
      view: string;
      session_id: string;
      turn?: number;
      eventId?: string;
      eval?: string;
    } & WithLayout)
  | ({ kind: "zoom"; view: string; mode?: string; zoom?: number } & WithLayout)
  | ({ kind: "view"; view: string } & WithLayout);

/** A navigation interaction from the current route. Coarse but enough to
 *  reconstruct "where you were" and replay the journey (finer kinds — filter/
 *  select/zoom — are emitted by the individual views). Parity with HashRoute
 *  so where_is_user can confirm detail tab, filters, file path, Live filters. */
export function interactionFromRoute(route: HashRoute): Extract<Interaction, { kind: "navigate" }> {
  const p: Extract<Interaction, { kind: "navigate" }> = { kind: "navigate", view: route.view };
  if (route.sessionId) p.session_id = route.sessionId;
  if (route.detailView) p.detailView = route.detailView;
  if (route.eventId) p.eventId = route.eventId;
  if (route.filePath) p.filePath = route.filePath;
  if (route.searchQuery) p.searchQuery = route.searchQuery;
  if (route.userFilter) p.userFilter = route.userFilter;
  if (route.timeFilter) p.timeFilter = route.timeFilter;
  if (route.explore?.filters && Object.keys(route.explore.filters).length > 0) {
    p.filters = route.explore.filters;
  }
  if (route.reelId) p.reelId = route.reelId;
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

/** A glass-ink interaction — ink landed on the CURRENT CONTEXT's glass (human
 *  freehand or an agent `draw` with scope "here"). Built from the route so the
 *  frame it becomes keeps session/detail context: glass ink is deictic, and an
 *  agent reading `where_is_user` must still see *what* the ink points at. */
export function glassInkInteraction(
  route: HashRoute,
  key: string,
  strokeCount: number,
): Extract<Interaction, { kind: "navigate" }> {
  return { ...interactionFromRoute(route), glassInk: { key, stroke_count: strokeCount } };
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

/**
 * After paint: measure layout eyes and re-post the interaction with `layout`.
 * Double-rAF + short timeout so virtualized lists can mount the focused row.
 * Best-effort — never throws, never blocks navigation.
 */
export function postInteractionWithLayoutEyes(
  base: Interaction,
  opts?: {
    readonly baseUrl?: string;
    readonly preferId?: string;
    readonly preferKind?: string;
    readonly delayMs?: number;
    /** Called when glass targets were measured (for merging pen + layout). */
    readonly onLayout?: (layout: InteractionLayout) => void;
  },
): () => void {
  postInteraction(base, opts?.baseUrl);
  if (typeof window === "undefined") return () => {};

  let cancelled = false;
  const delay = opts?.delayMs ?? 80;
  const preferId =
    opts?.preferId ??
    ("eventId" in base && typeof base.eventId === "string" ? base.eventId : undefined) ??
    ("session_id" in base && typeof base.session_id === "string" ? base.session_id : undefined);
  const preferKind =
    opts?.preferKind ??
    (preferId && "eventId" in base && base.eventId === preferId
      ? "event"
      : preferId
        ? "session"
        : undefined);

  const run = () => {
    if (cancelled) return;
    void import("@/lib/layout-eyes").then(({ collectLayoutEyes, layoutEyesToWire }) => {
      if (cancelled) return;
      const eyes = collectLayoutEyes({ preferId, preferKind });
      if (eyes.targets.length === 0) return;
      const layout = layoutEyesToWire(eyes);
      opts?.onLayout?.(layout);
      postInteraction(
        { ...base, layout },
        opts?.baseUrl,
      );
    });
  };

  // Two frames for React commit, then a short delay for virtualizers/scroll-into-view.
  let raf2 = 0;
  const raf1 = window.requestAnimationFrame(() => {
    raf2 = window.requestAnimationFrame(() => {
      window.setTimeout(run, delay);
    });
  });

  return () => {
    cancelled = true;
    window.cancelAnimationFrame(raf1);
    if (raf2) window.cancelAnimationFrame(raf2);
  };
}
