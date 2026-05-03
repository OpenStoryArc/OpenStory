/**
 * SessionHeader — context strip above the Live timeline when a session
 * is selected.
 *
 * Shows: @user · ⌂ host · project · branch. When the session's user
 * differs from the local resolver's user, prepends a colored band
 * with "Replicated from another machine" — answers the recurring
 * "wait, whose session is this?" question without requiring the user
 * to drill into Explore.
 *
 * Renders nothing when no session is selected.
 */

import { useEffect, useState } from "react";
import { personColor } from "@/lib/person-color";
import { projectColor } from "@/lib/project-color";

export interface SessionHeaderInfo {
  readonly session_id: string;
  readonly user: string | null;
  readonly host: string | null;
  readonly project_id: string | null;
  readonly project_name: string | null;
  readonly branch: string | null;
}

interface SessionHeaderProps {
  /** The selected session's identity + origin metadata. `null` hides the bar. */
  session: SessionHeaderInfo | null;
  /** Local OpenStory's resolved user (from `/api/local-info`). `null` while loading. */
  localUser: string | null;
}

export function SessionHeader({ session, localUser }: SessionHeaderProps) {
  if (!session) return null;

  const isCrossUser =
    !!localUser && !!session.user && session.user !== localUser;
  const userColor = session.user ? personColor(session.user) : "#565f89";
  const projectName = session.project_name ?? session.project_id;

  return (
    <div
      className="border-b border-[#2f3348]"
      data-testid="session-header"
      data-cross-user={isCrossUser ? "true" : "false"}
    >
      {/* Cross-user band — colored, narrow, only renders when applicable. */}
      {isCrossUser && (
        <div
          className="px-4 py-1 text-[10px] flex items-center gap-1.5"
          style={{
            backgroundColor: `${userColor}20`,
            color: userColor,
            borderBottom: `1px solid ${userColor}55`,
          }}
          data-testid="session-header-cross-user-band"
        >
          <span aria-hidden="true">↪</span>
          <span>
            Replicated from another machine — viewing{" "}
            <span className="font-semibold">@{session.user}</span>'s session
          </span>
        </div>
      )}

      {/* Main row: session origin facts. Always rendered when a session
          is selected, regardless of cross-user state. */}
      <div className="px-4 py-2 flex items-center gap-3 flex-wrap text-[11px]">
        {session.user && (
          <span
            className="inline-flex items-center gap-1.5 font-semibold"
            style={{ color: userColor }}
            title="Origin user"
          >
            <span
              className="w-5 h-5 rounded-full flex items-center justify-center text-[9px]"
              style={{ backgroundColor: `${userColor}30`, color: userColor }}
              aria-hidden="true"
            >
              {session.user.slice(0, 2).toUpperCase()}
            </span>
            @{session.user}
          </span>
        )}
        {session.host && (
          <span
            className="text-[10px] text-[#7dcfff] bg-[#7dcfff15] px-1.5 py-0.5 rounded"
            title="Origin host"
          >
            ⌂ {session.host}
          </span>
        )}
        {projectName && (
          <span
            className="text-[10px] px-1.5 py-0.5 rounded"
            style={{
              color: projectColor(projectName),
              backgroundColor: `${projectColor(projectName)}18`,
              border: `1px solid ${projectColor(projectName)}33`,
            }}
            title="Project"
          >
            {projectName}
          </span>
        )}
        {session.branch && (
          <span className="text-[10px] text-[#9ece6a]" title="Git branch">
            {session.branch}
          </span>
        )}
        <span className="ml-auto text-[10px] text-[#414868] font-mono" title="Session ID">
          {session.session_id.slice(0, 8)}
        </span>
      </div>
    </div>
  );
}

/**
 * Convenience hook: look up a session by id from /api/sessions and
 * shape it into the SessionHeader's input. Returns `null` until the
 * fetch resolves; null while there's no `sessionId` to look up.
 */
export function useSessionHeaderInfo(
  sessionId: string | null,
): SessionHeaderInfo | null {
  const [info, setInfo] = useState<SessionHeaderInfo | null>(null);

  useEffect(() => {
    if (!sessionId) {
      setInfo(null);
      return;
    }
    const ctrl = new AbortController();
    fetch(`/api/sessions`, { signal: ctrl.signal })
      .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
      .then((data) => {
        if (ctrl.signal.aborted) return;
        const row = (data?.sessions ?? []).find(
          (s: { session_id: string }) => s.session_id === sessionId,
        );
        if (row) {
          setInfo({
            session_id: row.session_id,
            user: row.user ?? null,
            host: row.host ?? null,
            project_id: row.project_id ?? null,
            project_name: row.project_name ?? null,
            branch: row.branch ?? null,
          });
        } else {
          setInfo(null);
        }
      })
      .catch(() => {
        if (!ctrl.signal.aborted) setInfo(null);
      });
    return () => ctrl.abort();
  }, [sessionId]);

  return info;
}
