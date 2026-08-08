/** Pure hash-route parser and builder for deep-link navigation. */

import type { DetailView } from "@/components/explore/ExploreView";
import type { OverviewFilters, SortKey } from "@/lib/sessions-overview";

const VALID_SORTS = new Set<SortKey>(["recent", "events", "tokens", "duration"]);
const VALID_RANGES = new Set(["today", "7d", "30d"]);
/** Facet filter keys carried 1:1 as query params (search uses `q`). */
const OVERVIEW_FACET_KEYS = ["project", "host", "user", "branch", "status", "agent", "day"] as const;

/** Bookmarkable filter state carried as a query tail on ANY explore route —
 *  filters survive selecting a session or switching detail tabs. The path owns
 *  the session id; on explore paths `q` always means the sidebar text filter
 *  (the semantic-search query keeps its dedicated #/search?q= shortcut). */
export interface ExploreQuery {
  filters: OverviewFilters;
  sort?: SortKey;
}

/** Parse the explore filter query tail, or null if none set. */
function parseExploreQuery(params: URLSearchParams | null): ExploreQuery | null {
  if (!params) return null;
  const filters: OverviewFilters = {};
  for (const key of OVERVIEW_FACET_KEYS) {
    const v = params.get(key);
    if (v) filters[key] = v;
  }
  const range = params.get("range");
  if (range && VALID_RANGES.has(range)) filters.range = range as OverviewFilters["range"];
  const q = params.get("q");
  if (q) filters.search = q;

  const rawSort = params.get("sort");
  const sort = rawSort && VALID_SORTS.has(rawSort as SortKey) ? (rawSort as SortKey) : undefined;

  if (Object.keys(filters).length === 0 && !sort) return null;
  return { filters, ...(sort ? { sort } : {}) };
}

/** Serialize an ExploreQuery into URLSearchParams (stable key order). */
function buildExploreQuery(e: ExploreQuery): URLSearchParams {
  const params = new URLSearchParams();
  for (const key of OVERVIEW_FACET_KEYS) {
    const v = e.filters[key];
    if (v) params.set(key, v);
  }
  if (e.filters.range) params.set("range", e.filters.range);
  if (e.filters.search) params.set("q", e.filters.search);
  if (e.sort) params.set("sort", e.sort);
  return params;
}

export interface HashRoute {
  view: "live" | "explore" | "story" | "canvas" | "ask" | "users" | "admin" | "reels";
  sessionId?: string;
  detailView?: DetailView;
  eventId?: string;
  filePath?: string;
  searchQuery?: string;
  /** Optional user filter for the Live tab — bookmarkable & shareable.
   *  Wire format: `#/live?user=katie` (URLSearchParams-style query
   *  appended to the hash). When set, the Live sidebar narrows to
   *  sessions stamped with this user. */
  userFilter?: string;
  /** Bookmarkable Explore filter state — query tail on any explore route. */
  explore?: ExploreQuery;
  /** Optional time-window filter for the Live tab. Same query-tail
   *  pattern as `userFilter`: `#/live?time=today`. Composes with the
   *  user filter (logical AND). Valid values: "1h", "today", "week",
   *  "all" — anything else is silently dropped on parse. */
  timeFilter?: "1h" | "today" | "week" | "all";
  /**
   * Story: open ▾ details on the focused turn card (`#/story/SES/event/ID?details=1`).
   * Click-parity: agent can expand sentence depth without a human click.
   */
  storyDetails?: boolean;
  /** Story: expand eval-apply detail under the turn (`&eval=1`). */
  storyEvalOpen?: boolean;
  /** Story: expand event-id list under the turn (`&events=1`). */
  storyEventsOpen?: boolean;
  /**
   * Story: which apply-row outputs are open inside eval-apply
   * (`&apply=0,2` or `&apply=all`). Indices are 0-based apply order.
   */
  storyApplyOpen?: readonly number[] | "all";
  /**
   * Story: which nested agent (subagent) CycleCards are expanded
   * (`&agents=agent-id1,agent-id2`). Keys are agent session ids.
   */
  storyAgentOpen?: readonly string[];
  /** Reels: the selected reel (`#/reels/REEL_ID`). */
  reelId?: string;
  /** Reels: autoplay the selected reel (`&autoplay=1`). Only meaningful
   *  alongside `reelId` — reels routes never carry a sessionId. */
  reelAutoplay?: boolean;
}

/** Parse `agents` query value → sorted unique agent session ids. */
export function parseAgentOpenParam(
  raw: string | null | undefined,
): readonly string[] | undefined {
  if (raw == null || raw === "") return undefined;
  const out = [
    ...new Set(
      raw
        .split(",")
        .map((s) => s.trim())
        .filter((s) => s.length > 0),
    ),
  ].sort();
  return out.length > 0 ? out : undefined;
}

/** Serialize agent-open keys for the hash query. */
export function buildAgentOpenParam(
  open: readonly string[] | undefined,
): string | undefined {
  if (!open || open.length === 0) return undefined;
  const out = [...new Set(open.map((k) => k.trim()).filter(Boolean))].sort();
  return out.length > 0 ? out.join(",") : undefined;
}

/** Is this agent session forced open by Attention/hash? */
export function isAgentSessionOpen(
  force: readonly string[] | undefined,
  agentSessionId: string | null | undefined,
): boolean {
  if (!force || force.length === 0 || !agentSessionId) return false;
  const id = agentSessionId.trim();
  if (!id) return false;
  return force.includes(id);
}

/** Parse `apply` query value → open indices or "all". */
export function parseApplyOpenParam(
  raw: string | null | undefined,
): readonly number[] | "all" | undefined {
  if (raw == null || raw === "") return undefined;
  const trimmed = raw.trim();
  if (trimmed.toLowerCase() === "all") return "all";
  const parts = trimmed.split(",").map((s) => s.trim()).filter(Boolean);
  const idxs: number[] = [];
  for (const p of parts) {
    if (p.toLowerCase() === "all") return "all";
    const n = Number(p);
    if (Number.isInteger(n) && n >= 0) idxs.push(n);
  }
  if (idxs.length === 0) return undefined;
  return [...new Set(idxs)].sort((a, b) => a - b);
}

/** Serialize apply-open for the hash query. */
export function buildApplyOpenParam(
  open: readonly number[] | "all" | undefined,
): string | undefined {
  if (open === "all") return "all";
  if (!open || open.length === 0) return undefined;
  return [...new Set(open)].sort((a, b) => a - b).join(",");
}

/** Is apply-row `index` forced open by Attention/hash? */
export function isApplyOutputOpen(
  force: readonly number[] | "all" | undefined,
  index: number,
): boolean {
  if (force === "all") return true;
  if (!force) return false;
  return force.includes(index);
}

const VALID_VIEWS = new Set(["live", "explore", "story", "canvas", "ask", "users", "admin", "reels"]);
const VALID_DETAIL_VIEWS = new Set(["events", "conversation", "plans", "graph", "search"]);

/** Strip the `?key=value&…` tail from a hash and return [path, params]. */
function splitQuery(hash: string): [string, URLSearchParams | null] {
  const qIdx = hash.indexOf("?");
  if (qIdx < 0) return [hash, null];
  return [hash.slice(0, qIdx), new URLSearchParams(hash.slice(qIdx + 1))];
}

/** Parse window.location.hash into a HashRoute. */
export function parseHash(hash: string): HashRoute {
  const raw = hash.startsWith("#") ? hash.slice(1) : hash;

  // Handle search shortcut: /search?q=...
  if (raw.startsWith("/search")) {
    const qIdx = raw.indexOf("?");
    const params = qIdx >= 0 ? new URLSearchParams(raw.slice(qIdx + 1)) : null;
    const q = params?.get("q") ?? undefined;
    return { view: "explore", detailView: "search", ...(q ? { searchQuery: q } : {}) };
  }

  // Pull the query tail off before splitting on `/` — userFilter and any
  // future query-style options live there to keep the path readable.
  const [path, queryParams] = splitQuery(raw);
  const userFilter = queryParams?.get("user") || undefined;
  const rawTime = queryParams?.get("time");
  const timeFilter: HashRoute["timeFilter"] | undefined =
    rawTime === "1h" || rawTime === "today" || rawTime === "week" || rawTime === "all"
      ? rawTime
      : undefined;

  const parts = path.split("/").filter(Boolean);

  // Legacy alias: the Overview tab merged into Explore. Old #/overview links
  // (docs, bookmarks, agent open_view calls) land on Explore with their
  // filters intact; the legacy sid= param becomes the path-style session id.
  // Legacy aliases: the Heatmap tab became a Canvas mode; Lab's graduated
  // shapes live in Canvas too. (Storm was removed — preserved on the
  // ui-improvements branch — so #/storm falls through to the live default.)
  if (parts[0] === "heatmap" || parts[0] === "lab") {
    return { view: "canvas" };
  }

  if (parts[0] === "overview") {
    const explore = parseExploreQuery(queryParams);
    const sid = queryParams?.get("sid") || undefined;
    return {
      view: "explore",
      ...(sid ? { sessionId: sid } : {}),
      ...(explore ? { explore } : {}),
    };
  }

  const view = VALID_VIEWS.has(parts[0] ?? "")
    ? (parts[0] as "live" | "explore" | "story" | "canvas" | "ask" | "users" | "admin" | "reels")
    : "live";

  if (view === "users" || view === "admin" || view === "canvas" || view === "ask") {
    return { view };
  }

  // Reels: #/reels, #/reels/REEL_ID, #/reels/REEL_ID?autoplay=1 — a
  // standalone spine that never carries sessionId, so it's handled before
  // the generic live/story session-id branch below.
  if (view === "reels") {
    const route: HashRoute = { view };
    if (parts[1]) route.reelId = parts[1];
    if (route.reelId && queryParams?.get("autoplay") === "1") route.reelAutoplay = true;
    return route;
  }

  if (view === "live" || view === "story") {
    const sessionId = parts[1] || undefined;
    const route: HashRoute = { view };
    if (sessionId) route.sessionId = sessionId;
    // Story deep-links to one event's turn (#/story/SES/event/ID) — the
    // event→turn canopy edge. StoryView consumes route.eventId to scroll.
    if (view === "story" && sessionId && parts[2] === "event" && parts[3]) {
      route.eventId = parts[3];
    }
    if (userFilter) route.userFilter = userFilter;
    if (timeFilter) route.timeFilter = timeFilter;
    // Story expand flags — agent click-parity for turn card interiors.
    if (view === "story" && queryParams) {
      if (queryParams.get("details") === "1") route.storyDetails = true;
      if (queryParams.get("eval") === "1") route.storyEvalOpen = true;
      if (queryParams.get("events") === "1") route.storyEventsOpen = true;
      const applyOpen = parseApplyOpenParam(queryParams.get("apply"));
      if (applyOpen !== undefined) {
        route.storyApplyOpen = applyOpen;
        // Apply expand implies the parent eval-apply + details panels.
        route.storyDetails = true;
        route.storyEvalOpen = true;
      }
      const agentOpen = parseAgentOpenParam(queryParams.get("agents"));
      if (agentOpen !== undefined) {
        route.storyAgentOpen = agentOpen;
        // Nested agent expand implies parent details + eval-apply panels.
        route.storyDetails = true;
        route.storyEvalOpen = true;
      }
    }
    return route;
  }

  // explore — the filter query tail rides along on every path shape.
  const explore = parseExploreQuery(queryParams);
  const withQuery = (route: HashRoute): HashRoute => (explore ? { ...route, explore } : route);

  const sessionId = parts[1] || undefined;
  if (!sessionId) return withQuery({ view });

  const segment2 = parts[2];

  // /explore/SES/event/EVT — an event deep-link lands on the Events detail
  // view, the one place that scrolls to + auto-expands the exact event
  // (landing on the session view left the zoom machinery unmounted).
  if (segment2 === "event" && parts[3]) {
    return withQuery({ view, sessionId, eventId: parts[3], detailView: "events" });
  }

  // /explore/SES/file/ENCODED_PATH
  if (segment2 === "file" && parts[3]) {
    return withQuery({ view, sessionId, filePath: decodeURIComponent(parts[3]) });
  }

  // /explore/SES/detailView
  if (segment2 && VALID_DETAIL_VIEWS.has(segment2)) {
    return withQuery({ view, sessionId, detailView: segment2 as DetailView });
  }

  return withQuery({ view, sessionId });
}

/** Build a hash string from a HashRoute. */
export function buildHash(route: HashRoute): string {
  // Search shortcut with query
  if (route.detailView === "search" && route.searchQuery) {
    return `#/search?q=${route.searchQuery.replace(/ /g, "+")}`;
  }

  const parts: string[] = [route.view];

  if (route.view === "reels") {
    // Reels routes never carry sessionId — handled first so the generic
    // session-id branch below never runs for this view.
    if (route.reelId) parts.push(route.reelId);
  } else if (route.sessionId) {
    parts.push(route.sessionId);

    if (route.eventId) {
      parts.push("event", route.eventId);
    } else if (route.filePath) {
      parts.push("file", encodeURIComponent(route.filePath));
    } else if (route.detailView) {
      parts.push(route.detailView);
    }
  } else if (route.detailView) {
    // No session, but has detail view (e.g., explore/search)
    parts.push(route.detailView);
  }

  // Append query tail for non-path options. `userFilter` and
  // `timeFilter` live here today; future options should follow the
  // same pattern rather than adding more path segments. Only Live
  // and Story honour these — the other tabs ignore them.
  let params = new URLSearchParams();
  if (route.view === "live" || route.view === "story") {
    if (route.userFilter) params.set("user", route.userFilter);
    if (route.timeFilter && route.timeFilter !== "all") {
      // "all" is the implicit default — omit it to keep the URL clean.
      params.set("time", route.timeFilter);
    }
    if (route.view === "story") {
      if (route.storyDetails) params.set("details", "1");
      if (route.storyEvalOpen) params.set("eval", "1");
      if (route.storyEventsOpen) params.set("events", "1");
      const applyParam = buildApplyOpenParam(route.storyApplyOpen);
      if (applyParam) params.set("apply", applyParam);
      const agentsParam = buildAgentOpenParam(route.storyAgentOpen);
      if (agentsParam) params.set("agents", agentsParam);
    }
  }
  // Explore's filter state rides every explore path so filters survive
  // selecting a session or switching detail tabs.
  if (route.view === "explore" && route.explore) {
    params = buildExploreQuery(route.explore);
  }
  if (route.view === "reels" && route.reelAutoplay) {
    params.set("autoplay", "1");
  }
  const query = params.toString() ? `?${params.toString()}` : "";

  return "#/" + parts.join("/") + query;
}
