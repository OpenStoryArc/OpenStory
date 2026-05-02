/** Pure hash-route parser and builder for deep-link navigation. */

import type { DetailView } from "@/components/explore/ExploreView";

export interface HashRoute {
  view: "live" | "explore" | "story" | "users";
  sessionId?: string;
  detailView?: DetailView;
  eventId?: string;
  filePath?: string;
  searchQuery?: string;
}

const VALID_VIEWS = new Set(["live", "explore", "story", "users"]);
const VALID_DETAIL_VIEWS = new Set(["events", "conversation", "plans", "search"]);

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

  const parts = raw.split("/").filter(Boolean);
  const view = VALID_VIEWS.has(parts[0] ?? "")
    ? (parts[0] as "live" | "explore" | "story" | "users")
    : "live";

  if (view === "users") {
    return { view };
  }

  if (view === "live" || view === "story") {
    const sessionId = parts[1] || undefined;
    return sessionId ? { view, sessionId } : { view };
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

  return "#/" + parts.join("/");
}
