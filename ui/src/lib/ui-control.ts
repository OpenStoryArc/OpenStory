/** Pure interpreter for agent "view intents" (control messages) → typed UI
 *  actions. The write side of the agent-in-UI seam: an MCP/operator posts to
 *  /api/control, the server broadcasts a `control` message, and the UI reacts.
 *  This module owns the CONTROL VOCABULARY — what an agent can ask the dashboard
 *  to do — kept pure so coverage of the problem space is tested independently of
 *  the React/WS boundary. It only steers what the dashboard shows, never the
 *  observed sources ("drive the mirror, never the watched"). */

import { parseHash, type HashRoute } from "@/lib/hash-route";
import type { OverviewFilters, SortKey } from "@/lib/sessions-overview";
import { planNavigateTo, type ControlStep, type NavigateToParams } from "@/lib/nav-path";

/** Facet keys an agent can filter by (Query class + open_view structured params). */
const QUERY_KEYS = ["project", "agent", "user", "status", "host", "branch", "day"] as const;
const SORTS: readonly SortKey[] = ["recent", "events", "tokens", "duration"];
const RANGES = new Set(["today", "7d", "30d"]);
const TIME_FILTERS = new Set(["1h", "today", "week", "all"]);

/** Full verb set the MCP schema and docs should advertise. */
export const CONTROL_VERBS = [
  "open_view",
  "focus_event",
  "navigate_to",
  "present",
  "announce",
  "highlight",
  "query",
  "filter",
  "set_filter",
  "toggle",
  "set",
] as const;

export interface ControlParams {
  route?: string;
  view?: string;
  sessionId?: string;
  eventId?: string;
  detailView?: string;
  reelId?: string;
  autoplay?: unknown;
  filePath?: string;
  searchQuery?: string;
  userFilter?: string;
  timeFilter?: string;
  message?: string;
  note?: string;
  sessionIds?: unknown;
  /** focus_event: true = full-screen presentation spotlight of the event.
   *  present: true = full-screen TITLE CARD of the message. */
  spotlight?: unknown;
  [k: string]: unknown;
}

/** Pull OverviewFilters + optional sort from flat params (query / open_view). */
function exploreFromParams(p: Record<string, unknown>): HashRoute["explore"] | undefined {
  const filters: OverviewFilters = {};
  for (const k of QUERY_KEYS) {
    const v = p[k];
    if (typeof v === "string" && v.trim()) filters[k] = v.trim();
  }
  const search = typeof p.search === "string" ? p.search : typeof p.q === "string" ? p.q : "";
  if (search.trim()) filters.search = search.trim();
  if (typeof p.range === "string" && RANGES.has(p.range)) {
    filters.range = p.range as OverviewFilters["range"];
  }
  const sort =
    typeof p.sort === "string" && SORTS.includes(p.sort as SortKey)
      ? (p.sort as SortKey)
      : undefined;
  if (Object.keys(filters).length === 0 && !sort) return undefined;
  return { filters, ...(sort ? { sort } : {}) };
}

/** A typed UI action, ready to apply. The discriminated union grows as the
 *  control vocabulary covers more of the UI's surface. */
export type UIControlAction =
  | { readonly type: "navigate"; readonly route: HashRoute }
  | {
      readonly type: "present";
      readonly message: string;
      readonly sessionIds: readonly string[];
      /** optional deep-link the human can jump to from the banner. */
      readonly route: HashRoute | null;
    }
  | {
      /** Toggle a component-local view control (Canvas mode, Heatmap 2D/3D, …).
       *  `target` is a "view.control" key (e.g. "canvas.mode"); the owning view
       *  subscribes to control$ and validates/applies the value. */
      readonly type: "toggle";
      readonly target: string;
      readonly value: string;
    }
  | {
      /** Presentation spotlight: fill the screen with ONE event and dim
       *  everything else (narrated demos). Raised by `focus_event` with
       *  `spotlight: true`; dismissed by Esc / backdrop click, any subsequent
       *  view-changing action, or `toggle {target:"spotlight", value:"off"}`. */
      readonly type: "spotlight";
      readonly sessionId: string;
      readonly eventId: string;
      /** Presentation-side framing: render only the text BEFORE the first
       *  occurrence of this marker (e.g. a trailing "## Where that leaves us"
       *  section that doesn't belong in the shot). The record is untouched —
       *  this crops the camera, not the data. */
      readonly clipAt?: string;
    }
  | {
      /** Title card: the message itself fills the screen (narrated demos —
       *  openers and closers). Raised by `present` with `spotlight: true`;
       *  dismissed exactly like an event spotlight. */
      readonly type: "title";
      readonly message: string;
    }
  | {
      /** Set a STRUCTURED view control — the multi-field sibling of `toggle`,
       *  for targets a string can't express (a brush box, a camera pose, a
       *  drill path). `target` names the control; `params` carries the object.
       *  This is the type-driven substrate for composing view shapes. */
      readonly type: "set";
      readonly target: string;
      readonly params: Record<string, unknown>;
    }
  | {
      /**
       * High-level hand: planNav / planNavigateTo → ordered control steps.
       * App executes them in sequence (full click-parity entry point).
       */
      readonly type: "navigate_sequence";
      readonly steps: readonly ControlStep[];
    };

/** Resolve a hash `route` string ("#/explore/abc" | "/explore/abc" | "explore")
 *  to a HashRoute, tolerating a missing leading # or /. */
function resolveRoute(route: string): HashRoute {
  const hash = route.startsWith("#") ? route : `#${route.startsWith("/") ? "" : "/"}${route}`;
  return parseHash(hash);
}

/** For `open_view`, the navigation route (or null if malformed). Accepts a full
 *  hash `route` string (escape hatch for any bookmarkable state) or structured
 *  params covering every HashRoute field: view, sessionId, detailView, eventId,
 *  filePath, searchQuery, userFilter, timeFilter, explore filters/sort. */
export function controlToRoute(action: string, params: unknown): HashRoute | null {
  if (action !== "open_view") return null;
  const p = (params ?? {}) as ControlParams;
  if (typeof p.route === "string" && p.route.trim()) return resolveRoute(p.route);
  if (typeof p.view === "string" && p.view.trim()) {
    const r: HashRoute = { view: p.view as HashRoute["view"] };
    if (typeof p.sessionId === "string") r.sessionId = p.sessionId;
    if (typeof p.detailView === "string") r.detailView = p.detailView as HashRoute["detailView"];
    // Carry reelId/autoplay — planNavigateTo({kind:"reel", ...}) emits
    // {view:"reels", reelId, autoplay} (nav-path.ts), and every
    // planned-step consumer (runControlSequence fallback, foldSteps,
    // raw ui_control calls) goes through this open_view branch. Without
    // it, a planned reel navigation silently lands on the reels list.
    if (typeof p.reelId === "string" && p.reelId.trim()) r.reelId = p.reelId;
    if (p.autoplay === true) r.reelAutoplay = true;
    // Carry eventId so an open_view can deep-link to a single event — replay
    // retraces to the EXACT event the human was on, not just the session.
    if (typeof p.eventId === "string" && p.eventId.trim()) r.eventId = p.eventId;
    // Carry searchQuery so an agent can drive cross-session search — the
    // file→session edge lands on #/search?q=<path>.
    if (typeof p.searchQuery === "string" && p.searchQuery.trim()) {
      r.searchQuery = p.searchQuery;
    }
    if (typeof p.filePath === "string" && p.filePath.trim()) r.filePath = p.filePath;
    if (typeof p.userFilter === "string" && p.userFilter.trim()) r.userFilter = p.userFilter;
    if (typeof p.timeFilter === "string" && TIME_FILTERS.has(p.timeFilter)) {
      r.timeFilter = p.timeFilter as HashRoute["timeFilter"];
    }
    // Flat facet keys / sort / range compose into explore so open_view can land
    // on a filtered Explore without a separate query intent.
    const explore = exploreFromParams(p as Record<string, unknown>);
    if (explore) r.explore = explore;
    return r;
  }
  return null;
}

/** Interpret a control intent into a typed UI action, or null if unrecognized /
 *  malformed. This is the dispatch table for the whole control vocabulary. */
export function interpretControl(action: string, params: unknown): UIControlAction | null {
  if (action === "open_view") {
    const route = controlToRoute(action, params);
    return route ? { type: "navigate", route } : null;
  }
  // High-level: any entity / canvas → planned control sequence (agent primary hand).
  if (action === "navigate_to") {
    const p = (params ?? {}) as NavigateToParams & Record<string, unknown>;
    const kind = typeof p.kind === "string" ? p.kind.trim() : "";
    const id = typeof p.id === "string" ? p.id.trim() : kind === "canvas" ? "canvas" : "";
    if (!kind) return null;
    const steps = planNavigateTo({
      kind: kind as NavigateToParams["kind"],
      id: id || "canvas",
      sessionId: typeof p.sessionId === "string" ? p.sessionId : undefined,
      eventId: typeof p.eventId === "string" ? p.eventId : undefined,
      user: typeof p.user === "string" ? p.user : undefined,
      project: typeof p.project === "string" ? p.project : undefined,
      filePath: typeof p.filePath === "string" ? p.filePath : undefined,
      parentSessionId: typeof p.parentSessionId === "string" ? p.parentSessionId : undefined,
      view: p.view === "explore" || p.view === "story" ? p.view : undefined,
      details: p.details === true || p.details === "true" || p.expandAll === true,
      evalOpen: p.evalOpen === true || p.evalOpen === "true" || p.expandAll === true,
      eventsOpen: p.eventsOpen === true || p.eventsOpen === "true" || p.expandAll === true,
      expandAll: p.expandAll === true || p.expandAll === "true",
      canvasMode: typeof p.canvasMode === "string" ? p.canvasMode : undefined,
      spotlight: p.spotlight === true,
      day: typeof p.day === "string" ? p.day : undefined,
      agent: typeof p.agent === "string" ? p.agent : undefined,
      groupBy: typeof p.groupBy === "string" ? p.groupBy : undefined,
      metric: p.metric === "events" || p.metric === "tokens" ? p.metric : undefined,
    });
    if (!steps || steps.length === 0) return null;
    return { type: "navigate_sequence", steps };
  }
  // The "focus_event" class: navigate-to-THING — open exactly one event in a
  // detail view (Explore or Story), which both consume `route.eventId` to scroll
  // to + reveal it. The finest grain of the map principle, driveable by an agent.
  if (action === "focus_event") {
    const p = (params ?? {}) as ControlParams;
    const sessionId = typeof p.sessionId === "string" ? p.sessionId.trim() : "";
    const eventId = typeof p.eventId === "string" ? p.eventId.trim() : "";
    if (!sessionId || !eventId) return null;
    // `spotlight: true` upgrades the focus to presentation mode — a full-screen
    // overlay of that one event, instead of a navigation.
    if (p.spotlight === true) {
      const clipAt =
        typeof p.clipAt === "string" && p.clipAt.trim() ? p.clipAt : undefined;
      return { type: "spotlight", sessionId, eventId, clipAt };
    }
    const view: HashRoute["view"] = p.view === "story" ? "story" : "explore";
    return {
      type: "navigate",
      route:
        view === "explore"
          ? { view, sessionId, eventId, detailView: "events" }
          : { view, sessionId, eventId },
    };
  }
  // The "present" class: an agent shows the human something — a message and/or a
  // spotlight on sessions, optionally with a jump. `announce`/`highlight` are
  // aliases so callers can use whichever verb reads best.
  if (action === "present" || action === "announce" || action === "highlight") {
    const p = (params ?? {}) as ControlParams;
    const message =
      typeof p.message === "string" ? p.message : typeof p.note === "string" ? p.note : "";
    const sessionIds = Array.isArray(p.sessionIds)
      ? p.sessionIds.filter((x): x is string => typeof x === "string")
      : [];
    const route = typeof p.route === "string" && p.route.trim() ? resolveRoute(p.route) : null;
    if (!message.trim() && sessionIds.length === 0 && !route) return null;
    // `spotlight: true` upgrades the message to a full-screen title card —
    // the words themselves become the shot (demo openers/closers).
    if (p.spotlight === true && message.trim()) return { type: "title", message };
    return { type: "present", message, sessionIds, route };
  }
  // The "query" class: narrow the data. Resolves to a filtered Explore route
  // (Explore hydrates filters + sort straight from the route, so no extra UI
  // handler is needed). `filter`/`set_filter` are aliases. At least one facet
  // (or free-text / range) must be set — sort alone is not a query.
  if (action === "query" || action === "filter" || action === "set_filter") {
    const p = (params ?? {}) as Record<string, unknown>;
    const explore = exploreFromParams(p);
    if (!explore || Object.keys(explore.filters).length === 0) return null;
    return { type: "navigate", route: { view: "explore", explore } };
  }
  // The "toggle" class: set a component-local view control. `target` names the
  // control ("canvas.mode", "heatmap.dim", "story.sort"); the owning view
  // validates + applies. Value validation lives in the view, not here, so the
  // vocabulary stays open. `set` is an alias.
  if (action === "toggle") {
    const p = (params ?? {}) as { target?: unknown; value?: unknown };
    if (typeof p.target === "string" && p.target.trim() && p.value != null && p.value !== "") {
      return { type: "toggle", target: p.target.trim(), value: String(p.value) };
    }
    return null;
  }
  // Structured control: { target, ...fields } → set(target, params). The
  // remaining fields (minus target) become the typed object payload.
  if (action === "set") {
    const p = (params ?? {}) as Record<string, unknown>;
    const target = typeof p.target === "string" ? p.target.trim() : "";
    if (!target) return null;
    const { target: _t, ...rest } = p;
    void _t;
    return { type: "set", target, params: rest };
  }
  return null;
}
