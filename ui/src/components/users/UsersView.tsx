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
import { personColor } from "@/lib/person-color";
import { projectColor } from "@/lib/project-color";
import { ActivitySparkline } from "@/components/users/ActivitySparkline";
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
    // Two distinct empty states, same surface:
    //   - total === 0: brand-new install, no sessions yet
    //   - total > 0:  sessions on disk but none stamped (pre-PR-#42 data)
    const noData = total === 0;
    return (
      <div
        className="flex-1 min-h-0 overflow-y-auto p-8 flex flex-col items-center justify-start"
        data-testid="users-empty"
      >
        <div className="max-w-md text-center">
          <div
            className="w-16 h-16 mx-auto mb-4 rounded-full flex items-center justify-center text-2xl"
            style={{ backgroundColor: "#bb9af715", color: "#bb9af7" }}
            aria-hidden="true"
          >
            {noData ? "👋" : "🪪"}
          </div>
          <h2 className="text-base font-semibold text-[#c0caf5] mb-2">
            {noData
              ? "Welcome to OpenStory"
              : "No stamped users yet"}
          </h2>
          <p className="text-sm text-[#565f89] mb-3">
            {noData
              ? "Run an agent (Claude Code, pi-mono) and your sessions will appear here."
              : `${total} session${total === 1 ? "" : "s"} on disk, but none have a stamped user.`}
          </p>
          {!noData && (
            <p className="text-xs text-[#565f89] mb-4">
              Sessions captured before user stamping shipped don't appear here
              (we don't invent an "Unknown" bucket). Going forward:
            </p>
          )}
          {!noData && (
            <pre className="text-[11px] text-left bg-[#1a1b26] border border-[#2f3348] rounded p-3 text-[#9ece6a] mb-2 overflow-x-auto">
              {`export OPEN_STORY_USER=$USER\nexport OPEN_STORY_HOST=$(scutil --get LocalHostName)\ndocker compose ... up -d --force-recreate`}
            </pre>
          )}
          {!noData && (
            <p className="text-[10px] text-[#414868]">
              New events from this point on will be stamped and surface here.
            </p>
          )}
        </div>
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
              // Open Live tab with this user pre-selected as the filter.
              // URL becomes /#/live?user=katie — bookmarkable and shareable.
              onNavigate({ view: "live", userFilter: user.user })
            }
            className="text-base font-semibold text-[#bb9af7] hover:text-[#c0caf5] transition-colors"
            title={`View @${user.user}'s sessions in Live`}
            data-testid={`user-card-header-${user.user}`}
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
          <div className="mt-1.5 flex items-center gap-1 flex-wrap">
            <span className="text-[10px] text-[#414868]">projects:</span>
            {user.projects.slice(0, 4).map((p) => {
              const c = projectColor(p);
              return (
                <span
                  key={p}
                  className="text-[10px] px-1.5 py-0.5 rounded"
                  style={{
                    color: c,
                    backgroundColor: `${c}18`,
                    border: `1px solid ${c}33`,
                  }}
                  title={`Project: ${p}`}
                >
                  {p}
                </span>
              );
            })}
            {user.projects.length > 4 && (
              <span className="text-[10px] text-[#565f89]">
                +{user.projects.length - 4}
              </span>
            )}
          </div>
        )}
        {/* 24h activity sparkline — colored with the user's hue. */}
        <div className="mt-2" title="Event volume over the last 24 hours">
          <ActivitySparkline
            buckets={user.activity_24h}
            color={personColor(user.user)}
            height={28}
            ariaLabel={`${user.user}'s activity over the last 24 hours`}
          />
          <div className="flex justify-between text-[9px] text-[#414868] mt-0.5 px-0.5">
            <span>24h ago</span>
            <span>12h</span>
            <span>now</span>
          </div>
        </div>
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
