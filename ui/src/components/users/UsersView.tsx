/**
 * UsersView — top-level Users tab.
 *
 * Lists every distinct stamped user with a card showing:
 *   - aggregate counts (sessions, tokens),
 *   - hosts and projects they've worked from,
 *   - last activity (relative time),
 *   - 5 most-recent sessions with the first-prompt label as the
 *     deterministic "what they're doing" surface.
 *
 * Click a session row → navigate to Live tab focused on that session.
 * Click the user header → narrow Live tab via ?user=X.
 *
 * v0.1 — labels are the "what they're doing" stand-in. v1 (see backlog
 * "InsightExtraction consumer") swaps this for structured insights
 * without changing the surface.
 */

import { useEffect, useState } from "react";
import { fetchUsers, type UserSummary, type UsersResponse } from "@/lib/users-api";
import { compactTime } from "@/lib/time";
import { sessionColor } from "@/lib/session-colors";
import type { HashRoute } from "@/lib/hash-route";

interface UsersViewProps {
  onNavigate: (route: HashRoute) => void;
}

export function UsersView({ onNavigate }: UsersViewProps) {
  const [data, setData] = useState<UsersResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const ctrl = new AbortController();
    setLoading(true);
    fetchUsers()
      .then((r) => {
        if (ctrl.signal.aborted) return;
        setData(r);
        setError(null);
        setLoading(false);
      })
      .catch((err) => {
        if (ctrl.signal.aborted) return;
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });
    return () => ctrl.abort();
  }, []);

  if (loading) {
    return (
      <div className="flex-1 min-h-0 overflow-y-auto p-6 text-sm text-[#565f89]">
        Loading users…
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex-1 min-h-0 overflow-y-auto p-6 text-sm text-[#f7768e]">
        Failed to load users: {error}
      </div>
    );
  }

  const users = data?.users ?? [];
  const total = data?.total ?? 0;
  const stamped = users.reduce((sum, u) => sum + u.session_count, 0);

  if (users.length === 0) {
    return (
      <div className="flex-1 min-h-0 overflow-y-auto p-6 text-sm text-[#565f89]" data-testid="users-empty">
        <p className="mb-2">No stamped users yet.</p>
        <p className="text-xs">
          Set <code className="px-1 bg-[#24283b] rounded">OPEN_STORY_USER</code> in
          your shell or <code className="px-1 bg-[#24283b] rounded">.env</code> and
          restart the container — new events will be stamped and appear here.
          Legacy sessions (without a user stamp) are intentionally excluded:{" "}
          {total} total session{total === 1 ? "" : "s"} on disk.
        </p>
      </div>
    );
  }

  return (
    <div className="flex-1 min-h-0 overflow-y-auto p-6" data-testid="users-view">
      <div className="mb-4 text-xs text-[#565f89]">
        {users.length} user{users.length === 1 ? "" : "s"} · {stamped} stamped
        session{stamped === 1 ? "" : "s"}
        {stamped < total && (
          <span> ({total - stamped} legacy unstamped not shown)</span>
        )}
      </div>
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {users.map((u) => (
          <UserCard key={u.user} user={u} onNavigate={onNavigate} />
        ))}
      </div>
    </div>
  );
}

interface UserCardProps {
  user: UserSummary;
  onNavigate: (route: HashRoute) => void;
}

function UserCard({ user, onNavigate }: UserCardProps) {
  const totalTokens = user.total_input_tokens + user.total_output_tokens;
  const lastActiveLabel = user.last_active
    ? compactTime(user.last_active)
    : "no activity";

  return (
    <div
      className="rounded-xl border border-[#2f3348] bg-[#1f2335] overflow-hidden"
      data-testid={`user-card-${user.user}`}
    >
      <div className="px-4 py-3 border-b border-[#2f3348]">
        <div className="flex items-baseline justify-between gap-2 mb-1">
          <button
            onClick={() =>
              // Click the user header → narrow Live tab via ?user= filter.
              // The Live tab's sidebar reads filter state from local UI
              // state today; pushing into a hash-route param would need
              // extra plumbing. For v0.1 we just send the user to Live;
              // they can then click any session to drill in.
              onNavigate({ view: "live" })
            }
            className="text-base font-semibold text-[#bb9af7] hover:text-[#c0caf5] transition-colors"
            title={`Open Live tab (filter by user manually for now)`}
          >
            @{user.user}
          </button>
          <span className="text-[10px] text-[#565f89]">{lastActiveLabel}</span>
        </div>
        <div className="flex items-center gap-3 text-[11px] text-[#565f89] flex-wrap">
          <span>
            {user.session_count} session{user.session_count === 1 ? "" : "s"}
          </span>
          {totalTokens > 0 && (
            <span title="Total tokens (input + output)">
              {formatTokens(totalTokens)} tokens
            </span>
          )}
          {user.hosts.length > 0 && (
            <span className="flex items-center gap-1 flex-wrap">
              {user.hosts.map((h) => (
                <span
                  key={h}
                  className="text-[10px] text-[#7dcfff] bg-[#7dcfff15] px-1.5 py-0.5 rounded"
                  title="Host machine"
                >
                  ⌂ {h}
                </span>
              ))}
            </span>
          )}
        </div>
        {user.projects.length > 0 && (
          <div className="mt-1.5 text-[10px] text-[#565f89] truncate">
            <span className="text-[#414868]">projects:</span>{" "}
            {user.projects.slice(0, 3).join(" · ")}
            {user.projects.length > 3 && (
              <span> +{user.projects.length - 3}</span>
            )}
          </div>
        )}
      </div>

      <div className="px-2 py-2">
        <div className="text-[10px] text-[#414868] uppercase tracking-wider px-2 py-1">
          Recent sessions
        </div>
        {user.recent_sessions.length === 0 ? (
          <div className="px-2 py-1 text-[11px] text-[#565f89] italic">
            No recent activity.
          </div>
        ) : (
          user.recent_sessions.map((s) => {
            const color = sessionColor(s.session_id);
            return (
              <button
                key={s.session_id}
                onClick={() =>
                  onNavigate({ view: "live", sessionId: s.session_id })
                }
                className="w-full text-left px-2 py-1.5 rounded hover:bg-[#24283b] transition-colors"
                data-testid={`user-card-${user.user}-session-${s.session_id.slice(0, 8)}`}
              >
                <div className="flex items-center gap-2">
                  <span
                    className="text-[9px] px-1 py-0.5 rounded shrink-0"
                    style={{ color, backgroundColor: `${color}20` }}
                  >
                    {s.session_id.slice(0, 8)}
                  </span>
                  <span className="text-[11px] text-[#c0caf5] truncate flex-1">
                    {s.label || (
                      <span className="text-[#565f89] italic">
                        no prompt yet
                      </span>
                    )}
                  </span>
                </div>
                <div className="flex items-center gap-2 mt-0.5 text-[10px] text-[#565f89]">
                  <span>{s.event_count} events</span>
                  {s.last_event && (
                    <span>{compactTime(s.last_event)}</span>
                  )}
                  {s.project_name && (
                    <span className="text-[#73daca] truncate">
                      {s.project_name}
                    </span>
                  )}
                </div>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}

/** Format a token count compactly: 1234 → "1.2K", 1234567 → "1.2M". */
function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}
