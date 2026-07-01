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
/** Facet filter keys carried 1:1 as query params (search uses `q`). */
const OVERVIEW_FACET_KEYS = ["project", "host", "user", "branch", "status", "agent", "day"] as const;

export interface HashRoute {
  view: "live" | "explore" | "story" | "overview" | "users" | "admin";
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
  /** Bookmarkable Overview dashboard state (filters/sort/drill-in). */
  overview?: OverviewRoute;
  /** Optional time-window filter for the Live tab. Same query-tail
   *  pattern as `userFilter`: `#/live?time=today`. Composes with the
   *  user filter (logical AND). Valid values: "1h", "today", "week",
   *  "all" — anything else is silently dropped on parse. */
  timeFilter?: "1h" | "today" | "week" | "all";
}

const VALID_VIEWS = new Set(["live", "explore", "story", "overview", "users", "admin"]);
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
    ? (parts[0] as "live" | "explore" | "story" | "overview" | "users" | "admin")
    : "live";

  if (view === "users" || view === "admin") {
    return { view };
  }

  if (view === "overview") {
    const overview = parseOverviewQuery(queryParams);
    return overview ? { view, overview } : { view };
  }

  if (view === "live" || view === "story") {
    const sessionId = parts[1] || undefined;
    const route: HashRoute = { view };
    if (sessionId) route.sessionId = sessionId;
    if (userFilter) route.userFilter = userFilter;
    if (timeFilter) route.timeFilter = timeFilter;
    return route;
  }

  // explore
  const sessionId = parts[1] || undefined;
  if (!sessionId) return { view };

  const segment2 = parts[2];

  // /explore/SES/event/EVT
  if (segment2 === "event" && parts[3]) {
    return { view, sessionId, eventId: parts[3] };
  }

  // /explore/SES/file/ENCODED_PATH
  if (segment2 === "file" && parts[3]) {
    return { view, sessionId, filePath: decodeURIComponent(parts[3]) };
  }

  // /explore/SES/detailView
  if (segment2 && VALID_DETAIL_VIEWS.has(segment2)) {
    return { view, sessionId, detailView: segment2 as DetailView };
  }

  return { view, sessionId };
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
  const params = new URLSearchParams();
  if (route.view === "live" || route.view === "story") {
    if (route.userFilter) params.set("user", route.userFilter);
    if (route.timeFilter && route.timeFilter !== "all") {
      // "all" is the implicit default — omit it to keep the URL clean.
      params.set("time", route.timeFilter);
    }
  }
  const query = params.toString() ? `?${params.toString()}` : "";

  return "#/" + parts.join("/") + query;
}
