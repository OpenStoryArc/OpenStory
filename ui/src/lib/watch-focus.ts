/** Agent-directed watch focus — the pure reception logic.
 *
 *  When the server pushes a `focus` message (from POST /api/watch/{id}),
 *  the UI decides what to do based on where the user currently is:
 *
 *    - On the Live tab → switch focus to the session instantly (zero clicks;
 *      the user is already watching, so this is the expected move).
 *    - On any other tab → surface a dismissible banner so the user is never
 *      yanked off what they're doing ("observe, never interfere" — applied to
 *      the human, not just the agent).
 *
 *  This module is side-effect-free; App.tsx performs the navigation or banner
 *  render based on the returned action. Named "watch focus" to distinguish it
 *  from `lib/focus.ts`, which is the unrelated event-subtree focus. */

import type { ViewMode } from "@/lib/navigation";
import type { FocusMessage, WsMessage } from "@/types/websocket";

/** What the UI should do in response to a focus message. */
export type WatchAction =
  | { type: "navigate"; sessionId: string }
  | { type: "banner"; message: FocusMessage };

/** Type guard narrowing a WsMessage to a FocusMessage. */
export function isFocusMessage(msg: WsMessage): msg is FocusMessage {
  return msg.kind === "focus";
}

/** Decide the reception action for a focus message given the current view. */
export function decideWatchAction(
  currentView: ViewMode,
  msg: FocusMessage,
): WatchAction {
  if (currentView === "live") {
    return { type: "navigate", sessionId: msg.session_id };
  }
  return { type: "banner", message: msg };
}
