/** Navigation types for Live/Explore tab switching and cross-linking. */

export type ViewMode = "live" | "explore" | "story" | "canvas" | "ask" | "lab" | "storm" | "users" | "admin";

/** Payload carried when cross-linking from Live → Explore. */
export interface CrossLink {
  readonly sessionId: string;
  readonly eventId?: string;
}

import type { HashRoute } from "@/lib/hash-route";
export type { HashRoute } from "@/lib/hash-route";

/** Views that render a specific session — switching between them carries
 *  the session along instead of dropping the user on an empty view. */
const SESSION_VIEWS: ReadonlySet<ViewMode> = new Set(["live", "explore", "story", "canvas"]);

/** The route a tab switch lands on: same view, and the current session
 *  carried across when the destination can show it (Live→Story keeps the
 *  session — the carry-session-across-tabs canopy edge). */
export function switchTabRoute(current: HashRoute, mode: ViewMode): HashRoute {
  return current.sessionId && SESSION_VIEWS.has(mode)
    ? { view: mode, sessionId: current.sessionId }
    : { view: mode };
}
