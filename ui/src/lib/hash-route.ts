/** Pure hash-route parser and builder for deep-link navigation. */

import type { DetailView } from "@/components/explore/ExploreView";
import type { OverviewFilters, SortKey } from "@/lib/sessions-overview";

/** Bookmarkable state for the Overview dashboard — filters + sort + drill-in. */
export interface OverviewRoute {
  filters: OverviewFilters;
  sort?: SortKey;
  /** Selected drill-in session id. */
  sessionId?: string;
}

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
  view: "live" | "explore" | "story" | "overview" | "canvas" | "ask" | "heatmap" | "lab" | "storm" | "users" | "admin";
  sessionId?: string;
  /** Storm board: deep-link one sticky (#/storm?sticky=id). */
  stickyId?: string;
  detailView?: DetailView;
  eventId?: string;
  filePath?: string;
  searchQuery?: string;
  /** Optional user filter for the Live tab — bookmarkable & shareable.
   *  Wire format: `#/live?user=katie` (URLSearchParams-style query
   *  appended to the hash). When set, the Live sidebar narrows to
   *  sessions stamped with this user. */
  userFilter?: string;
  /** Bookmarkable Overview dashboard state (filters/sort/drill-in). */
  overview?: OverviewRoute;
  /** Bookmarkable Explore filter state — query tail on any explore route. */
  explore?: ExploreQuery;
  /** Optional time-window filter for the Live tab. Same query-tail
   *  pattern as `userFilter`: `#/live?time=today`. Composes with the
   *  user filter (logical AND). Valid values: "1h", "today", "week",
   *  "all" — anything else is silently dropped on parse. */
  timeFilter?: "1h" | "today" | "week" | "all";
}

const VALID_VIEWS = new Set(["live", "explore", "story", "overview", "canvas", "ask", "heatmap", "lab", "storm", "users", "admin"]);
const VALID_DETAIL_VIEWS = new Set(["events", "conversation", "plans", "graph", "search"]);

/** Parse Overview query params into an OverviewRoute, or null if none set. */
function parseOverviewQuery(params: URLSearchParams | null): OverviewRoute | null {
  if (!params) return null;
  const filters: OverviewFilters = {};
  for (const key of OVERVIEW_FACET_KEYS) {
    const v = params.get(key);
    if (v) filters[key] = v;
  }
  const q = params.get("q");
  if (q) filters.search = q;

  const rawSort = params.get("sort");
  const sort = rawSort && VALID_SORTS.has(rawSort as SortKey) ? (rawSort as SortKey) : undefined;
  const sessionId = params.get("sid") || undefined;

  const hasFilters = Object.keys(filters).length > 0;
  if (!hasFilters && !sort && !sessionId) return null;
  return { filters, ...(sort ? { sort } : {}), ...(sessionId ? { sessionId } : {}) };
}

/** Serialize an OverviewRoute into URLSearchParams (stable key order). */
function buildOverviewQuery(o: OverviewRoute): URLSearchParams {
  const params = new URLSearchParams();
  for (const key of OVERVIEW_FACET_KEYS) {
    const v = o.filters[key];
    if (v) params.set(key, v);
  }
  if (o.filters.search) params.set("q", o.filters.search);
  if (o.sort) params.set("sort", o.sort);
  if (o.sessionId) params.set("sid", o.sessionId);
  return params;
}

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
  const view = VALID_VIEWS.has(parts[0] ?? "")
    ? (parts[0] as "live" | "explore" | "story" | "overview" | "canvas" | "ask" | "heatmap" | "lab" | "storm" | "users" | "admin")
    : "live";

  if (view === "users" || view === "admin" || view === "canvas" || view === "ask" || view === "heatmap" || view === "lab" || view === "storm") {
    // Storm deep-links a sticky — a shareable pointer at one architecture note.
    const sticky = view === "storm" ? queryParams?.get("sticky") : null;
    return sticky ? { view, stickyId: sticky } : { view };
  }

  if (view === "overview") {
    const overview = parseOverviewQuery(queryParams);
    return overview ? { view, overview } : { view };
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
    return route;
  }

  // explore — the filter query tail rides along on every path shape.
  const explore = parseExploreQuery(queryParams);
  const withQuery = (route: HashRoute): HashRoute => (explore ? { ...route, explore } : route);

  const sessionId = parts[1] || undefined;
  if (!sessionId) return withQuery({ view });

  const segment2 = parts[2];

  // /explore/SES/event/EVT
  if (segment2 === "event" && parts[3]) {
    return withQuery({ view, sessionId, eventId: parts[3] });
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

  // Overview dashboard with bookmarkable filter state.
  if (route.view === "overview" && route.overview) {
    const query = buildOverviewQuery(route.overview).toString();
    return query ? `#/overview?${query}` : "#/overview";
  }

  if (route.view === "storm" && route.stickyId) {
    return `#/storm?sticky=${encodeURIComponent(route.stickyId)}`;
  }

  const parts: string[] = [route.view];

  if (route.sessionId) {
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
  }
  // Explore's filter state rides every explore path so filters survive
  // selecting a session or switching detail tabs.
  if (route.view === "explore" && route.explore) {
    params = buildExploreQuery(route.explore);
  }
  const query = params.toString() ? `?${params.toString()}` : "";

  return "#/" + parts.join("/") + query;
}
