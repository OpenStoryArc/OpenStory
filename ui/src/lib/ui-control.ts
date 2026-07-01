/** Pure mapping from an agent "view intent" (control message) to a UI action.
 *  The write side of the agent-in-UI seam: an MCP/operator posts to
 *  /api/control, the server broadcasts a `control` message, and the UI reacts.
 *  This module decides WHAT to do — kept pure so the vocabulary is tested
 *  independently of the React/WS boundary. Only steers what the dashboard shows,
 *  never the observed sources ("drive the mirror, never the watched"). */

import { parseHash, type HashRoute } from "@/lib/hash-route";

export interface ControlParams {
  route?: string;
  view?: string;
  sessionId?: string;
  detailView?: string;
  [k: string]: unknown;
}

/** For `open_view`, resolve the navigation route (or null if the intent isn't a
 *  navigation or is malformed). Accepts either a hash `route` string
 *  ("#/explore/abc" or "/explore/abc") or structured `{ view, sessionId, … }`. */
export function controlToRoute(action: string, params: unknown): HashRoute | null {
  if (action !== "open_view") return null;
  const p = (params ?? {}) as ControlParams;

  if (typeof p.route === "string" && p.route.trim()) {
    const hash = p.route.startsWith("#") ? p.route : `#${p.route.startsWith("/") ? "" : "/"}${p.route}`;
    return parseHash(hash);
  }
  if (typeof p.view === "string" && p.view.trim()) {
    const r: HashRoute = { view: p.view as HashRoute["view"] };
    if (typeof p.sessionId === "string") r.sessionId = p.sessionId;
    if (typeof p.detailView === "string") r.detailView = p.detailView as HashRoute["detailView"];
    return r;
  }
  return null;
}
