/** Pure interpreter for agent "view intents" (control messages) → typed UI
 *  actions. The write side of the agent-in-UI seam: an MCP/operator posts to
 *  /api/control, the server broadcasts a `control` message, and the UI reacts.
 *  This module owns the CONTROL VOCABULARY — what an agent can ask the dashboard
 *  to do — kept pure so coverage of the problem space is tested independently of
 *  the React/WS boundary. It only steers what the dashboard shows, never the
 *  observed sources ("drive the mirror, never the watched"). */

import { parseHash, type HashRoute } from "@/lib/hash-route";
import type { OverviewFilters, SortKey } from "@/lib/sessions-overview";

/** Facet keys an agent can filter by (Query class). */
const QUERY_KEYS = ["project", "agent", "user", "status", "host", "branch", "day"] as const;
const SORTS: readonly SortKey[] = ["recent", "events", "tokens", "duration"];

export interface ControlParams {
  route?: string;
  view?: string;
  sessionId?: string;
  detailView?: string;
  message?: string;
  note?: string;
  sessionIds?: unknown;
  [k: string]: unknown;
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
      /** Set a STRUCTURED view control — the multi-field sibling of `toggle`,
       *  for targets a string can't express (a brush box, a camera pose, a
       *  drill path). `target` names the control; `params` carries the object.
       *  This is the type-driven substrate for composing view shapes. */
      readonly type: "set";
      readonly target: string;
      readonly params: Record<string, unknown>;
    };

/** Resolve a hash `route` string ("#/explore/abc" | "/explore/abc" | "explore")
 *  to a HashRoute, tolerating a missing leading # or /. */
function resolveRoute(route: string): HashRoute {
  const hash = route.startsWith("#") ? route : `#${route.startsWith("/") ? "" : "/"}${route}`;
  return parseHash(hash);
}

/** For `open_view`, the navigation route (or null if malformed). Accepts a hash
 *  `route` string or structured `{ view, sessionId, detailView }`. */
export function controlToRoute(action: string, params: unknown): HashRoute | null {
  if (action !== "open_view") return null;
  const p = (params ?? {}) as ControlParams;
  if (typeof p.route === "string" && p.route.trim()) return resolveRoute(p.route);
  if (typeof p.view === "string" && p.view.trim()) {
    const r: HashRoute = { view: p.view as HashRoute["view"] };
    if (typeof p.sessionId === "string") r.sessionId = p.sessionId;
    if (typeof p.detailView === "string") r.detailView = p.detailView as HashRoute["detailView"];
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
    return { type: "present", message, sessionIds, route };
  }
  // The "query" class: narrow the data. Resolves to a filtered Overview route
  // (Overview hydrates filters + sort straight from the route, so no extra UI
  // handler is needed). `filter`/`set_filter` are aliases.
  if (action === "query" || action === "filter" || action === "set_filter") {
    const p = (params ?? {}) as Record<string, unknown>;
    const filters: OverviewFilters = {};
    for (const k of QUERY_KEYS) {
      const v = p[k];
      if (typeof v === "string" && v.trim()) filters[k] = v.trim();
    }
    const search = typeof p.search === "string" ? p.search : typeof p.q === "string" ? p.q : "";
    if (search.trim()) filters.search = search.trim();
    if (Object.keys(filters).length === 0) return null;
    const overview: HashRoute["overview"] = { filters };
    if (typeof p.sort === "string" && SORTS.includes(p.sort as SortKey)) overview.sort = p.sort as SortKey;
    return { type: "navigate", route: { view: "overview", overview } };
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
