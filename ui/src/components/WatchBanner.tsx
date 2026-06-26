/** WatchBanner — shown when a focus message arrives while the user is NOT on
 *  the Live tab. The agent is watching a session; this offers to follow it
 *  without yanking the user off their current view. One click follows, one
 *  dismisses. It never navigates on its own — App.tsx owns the navigation.
 *
 *  Colors are Tokyonight tokens, matching the rest of the dashboard. */

import type { FocusMessage } from "@/types/websocket";

export function WatchBanner({
  message,
  onFollow,
  onDismiss,
}: {
  message: FocusMessage;
  onFollow: () => void;
  onDismiss: () => void;
}) {
  // Prefer the human-readable name; fall back to a short session id so the
  // banner is never empty even for an unenriched focus.
  const title =
    message.label ?? message.project_name ?? message.session_id.slice(0, 8);
  const origin = [message.host, message.user ? `@${message.user}` : null]
    .filter(Boolean)
    .join(" · ");

  return (
    <div
      data-testid="watch-banner"
      role="status"
      className="flex items-center gap-3 border-b border-[#7aa2f7]/30 bg-[#1f2335] px-4 py-2 text-sm text-[#c0caf5]"
    >
      <span aria-hidden className="text-[#7aa2f7]">
        👁
      </span>
      <span className="min-w-0 flex-1 truncate">
        <span className="text-[#565f89]">Agent is watching </span>
        <span className="font-medium text-[#c0caf5]">{title}</span>
        {message.project_name && message.project_name !== title && (
          <span className="text-[#565f89]"> · {message.project_name}</span>
        )}
        {origin && <span className="text-[#565f89]"> · {origin}</span>}
      </span>
      <button
        type="button"
        onClick={onFollow}
        className="shrink-0 rounded bg-[#7aa2f7] px-2.5 py-1 text-xs font-medium text-[#1a1b26] hover:bg-[#89b4fa]"
      >
        Follow →
      </button>
      <button
        type="button"
        onClick={onDismiss}
        aria-label="Dismiss"
        className="shrink-0 rounded px-2 py-1 text-xs text-[#565f89] hover:text-[#c0caf5]"
      >
        Dismiss
      </button>
    </div>
  );
}
