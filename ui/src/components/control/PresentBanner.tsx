/** PresentBanner — the "present" control class made visible: when an agent
 *  posts a `present`/`highlight`/`announce` intent, this strip shows its message
 *  and spotlights the sessions it pointed at, each click-through to Explore, with
 *  an optional jump. Dismissible — you can always wave the agent off. Part of the
 *  agent-in-UI seam; it only shows things, never mutates the observed sources. */

import type { HashRoute } from "@/lib/hash-route";

export interface Presentation {
  readonly issuer: string;
  readonly message: string;
  readonly sessionIds: readonly string[];
  readonly route: HashRoute | null;
}

export function PresentBanner({
  present,
  onNavigate,
  onDismiss,
}: {
  present: Presentation;
  onNavigate: (route: HashRoute) => void;
  onDismiss: () => void;
}) {
  const { issuer, message, sessionIds, route } = present;
  return (
    <div
      className="flex items-center gap-3 border-b border-[#7aa2f7]/40 bg-[#7aa2f7]/10 px-4 py-2 text-[13px] text-[#c0caf5]"
      data-testid="present-banner"
    >
      <span className="shrink-0 rounded bg-[#7aa2f7] px-1.5 py-0.5 text-[10px] font-semibold text-[#1a1b26]">
        ▸ {issuer}
      </span>
      {message && <span className="min-w-0 flex-1 truncate">{message}</span>}
      {sessionIds.length > 0 && (
        <div className="flex shrink-0 items-center gap-1.5">
          {sessionIds.slice(0, 6).map((id) => (
            <button
              key={id}
              data-present-session={id}
              onClick={() => onNavigate({ view: "explore", sessionId: id })}
              className="rounded border border-[#3b4261] px-1.5 py-0.5 font-mono text-[10px] text-[#7aa2f7] hover:border-[#7aa2f7] hover:bg-[#24283b]"
              title={id}
            >
              {id.slice(0, 8)}
            </button>
          ))}
          {sessionIds.length > 6 && (
            <span className="text-[10px] text-[#565f89]">+{sessionIds.length - 6}</span>
          )}
        </div>
      )}
      {route && (
        <button
          onClick={() => onNavigate(route)}
          className="shrink-0 rounded bg-[#7aa2f7] px-2 py-0.5 text-[11px] font-medium text-[#1a1b26] hover:bg-[#9db8fa]"
        >
          Open →
        </button>
      )}
      <button
        onClick={onDismiss}
        className="shrink-0 rounded px-1.5 text-[#565f89] hover:text-[#c0caf5]"
        title="Dismiss"
        aria-label="Dismiss"
      >
        ×
      </button>
    </div>
  );
}
