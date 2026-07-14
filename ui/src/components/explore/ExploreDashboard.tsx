/** ExploreDashboard — the Explore landing when no session is selected.
 *
 *  Overview's dashboard reborn inside Explore: stats bar (click a stat to
 *  sort), calendar heatmap over the full universe (click a day to filter),
 *  the frecency Recent strip, and honest error/empty states. The session
 *  LIST lives in the sidebar — this pane is the bird's-eye view.
 */

import { useState } from "react";
import type { StorySession } from "@/lib/story-api";
import type { OverviewStats, SortKey } from "@/lib/sessions-overview";
import { SessionCalendar } from "@/components/viz/SessionCalendar";
import { SessionListSkeleton } from "@/components/ui/skeletons";
import { sessionColor } from "@/lib/session-colors";
import { sessionTitle } from "@/lib/session-title";
import { cn } from "@/lib/cn";

function kfmt(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

function CopyLinkButton() {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() => {
        navigator.clipboard
          ?.writeText(window.location.href)
          .then(() => {
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1500);
          })
          .catch(() => {});
      }}
      className={cn(
        "rounded border px-2 py-1 text-[11px] transition-colors",
        copied ? "border-[color:var(--green)] text-[color:var(--green)]" : "border-[color:var(--border)] text-[color:var(--text-muted)] hover:border-[color:var(--accent)] hover:text-[color:var(--text)]",
      )}
      title="Copy a link to this filtered view"
    >
      {copied ? "✓ Copied" : "🔗 Copy link"}
    </button>
  );
}

interface Props {
  /** Full (non-subagent) session universe — the calendar always shows it all. */
  universe: readonly StorySession[];
  /** Aggregates over the FILTERED set, so the numbers match the sidebar list. */
  stats: OverviewStats;
  filtersActive: boolean;
  selectedDay: string | null;
  loading: boolean;
  error: string | null;
  refresh: () => void;
  recentSessions: readonly StorySession[];
  sortKey: SortKey;
  onSortKey: (k: SortKey) => void;
  onSelectDay: (day: string | null) => void;
  onOpenSession: (id: string) => void;
  onClearFilters: () => void;
}

export function ExploreDashboard({
  universe,
  stats,
  filtersActive,
  selectedDay,
  loading,
  error,
  refresh,
  recentSessions,
  sortKey,
  onSortKey,
  onSelectDay,
  onOpenSession,
  onClearFilters,
}: Props) {
  if (loading) {
    return (
      <div data-testid="explore-dashboard">
        <SessionListSkeleton />
      </div>
    );
  }

  if (error && universe.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 p-8 text-center" data-testid="explore-dashboard-error">
        <div className="text-[13px] text-[color:var(--red)]">Couldn't load sessions</div>
        <div className="max-w-[420px] break-words text-[11px] text-[color:var(--text-muted)]">{error}</div>
        <button
          onClick={refresh}
          className="mt-1 rounded border border-[color:var(--accent)] px-3 py-1 text-[11px] text-[color:var(--accent)] hover:bg-[#7aa2f7]/10"
        >
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col" data-testid="explore-dashboard">
      {/* Stats bar — numbers reflect the active filters; click a stat to sort. */}
      <div className="flex flex-wrap items-center gap-x-6 gap-y-1 border-b border-[color:var(--bg-hover)] px-4 py-2">
        <div>
          <div className="text-[18px] font-semibold tabular-nums text-[color:var(--text)]">{stats.sessionCount.toLocaleString()}</div>
          <div className="text-[10px] text-[color:var(--text-muted)]">sessions{filtersActive ? " (filtered)" : ""}</div>
        </div>
        <button onClick={() => onSortKey("events")} className="text-left hover:opacity-80" title="Sort by most events">
          <div className="text-[18px] font-semibold tabular-nums text-[color:var(--accent)]">{stats.eventCount.toLocaleString()}</div>
          <div className="text-[10px] text-[color:var(--text-muted)]">events{sortKey === "events" ? " ↓" : ""}</div>
        </button>
        <button onClick={() => onSortKey("tokens")} className="text-left hover:opacity-80" title="Sort by most tokens">
          <div className="text-[18px] font-semibold tabular-nums text-[color:var(--orange)]">{kfmt(stats.tokens)}</div>
          <div className="text-[10px] text-[color:var(--text-muted)]">tokens{sortKey === "tokens" ? " ↓" : ""}</div>
        </button>
        <div className="ml-auto flex min-w-0 flex-wrap items-center gap-4">
          <CopyLinkButton />
          {stats.busiest && (
            <button onClick={() => onOpenSession(stats.busiest!.session_id)} className="text-right hover:opacity-80">
              <div className="text-[11px] text-[color:var(--text-muted)]">busiest session</div>
              <div className="max-w-[280px] truncate text-[12px] text-[color:var(--text)]">
                {sessionTitle(stats.busiest)} · <span className="text-[color:var(--accent)]">{(stats.busiest.event_count ?? 0).toLocaleString()} ev</span>
              </div>
            </button>
          )}
        </div>
      </div>

      {/* Calendar — always the full universe so the shape of your history stays visible. */}
      <div className="overflow-x-auto border-b border-[color:var(--bg-hover)] px-4 py-3">
        <SessionCalendar sessions={[...universe]} selectedDay={selectedDay} onSelectDay={onSelectDay} />
      </div>

      {/* Recent strip — frecency where the eye already is. */}
      {recentSessions.length > 0 && (
        <div className="flex items-center gap-2 overflow-x-auto border-b border-[color:var(--bg-hover)] px-3 py-1.5" data-testid="recent-strip">
          <span className="shrink-0 text-[10px] uppercase tracking-wide text-[color:var(--text-muted)]">Recent</span>
          {recentSessions.map((s) => (
            <button
              key={s.session_id}
              data-recent-session={s.session_id}
              onClick={() => onOpenSession(s.session_id)}
              className="flex shrink-0 items-center gap-1.5 rounded-full border border-[color:var(--bg-hover)] px-2 py-0.5 text-[11px] text-[color:var(--text)] hover:border-[color:var(--accent)] hover:bg-[color:var(--bg-surface)]"
              title={sessionTitle(s)}
            >
              <span className="h-2 w-2 rounded-full" style={{ background: sessionColor(s.session_id) }} />
              <span className="max-w-[160px] truncate">{sessionTitle(s)}</span>
            </button>
          ))}
        </div>
      )}

      {/* Body — a hint (or an honest empty state when filters match nothing). */}
      {stats.sessionCount === 0 ? (
        <div className="flex flex-col items-center gap-2 p-8 text-center">
          <div className="text-[13px] text-[color:var(--text)]">No sessions match these filters</div>
          <div className="text-[11px] text-[color:var(--text-muted)]">
            {universe.length.toLocaleString()} sessions · try widening your search
          </div>
          {filtersActive && (
            <button
              onClick={onClearFilters}
              className="mt-1 rounded border border-[color:var(--accent)] px-3 py-1 text-[11px] text-[color:var(--accent)] hover:bg-[#7aa2f7]/10"
            >
              Reset filters
            </button>
          )}
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center p-8 text-center text-[color:var(--text-muted)]">
          <div>
            <div className="mb-2 text-lg">Select a session</div>
            <div className="text-xs">Pick one from the sidebar — filters and the calendar narrow the list.</div>
          </div>
        </div>
      )}
    </div>
  );
}
