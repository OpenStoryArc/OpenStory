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
        copied ? "border-[#9ece6a] text-[#9ece6a]" : "border-[#3b4261] text-[#565f89] hover:border-[#7aa2f7] hover:text-[#c0caf5]",
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
        <div className="text-[13px] text-[#f7768e]">Couldn't load sessions</div>
        <div className="max-w-[420px] break-words text-[11px] text-[#565f89]">{error}</div>
        <button
          onClick={refresh}
          className="mt-1 rounded border border-[#7aa2f7] px-3 py-1 text-[11px] text-[#7aa2f7] hover:bg-[#7aa2f7]/10"
        >
          Retry
        </button>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col" data-testid="explore-dashboard">
      {/* Stats bar — numbers reflect the active filters; click a stat to sort. */}
      <div className="flex flex-wrap items-center gap-x-6 gap-y-1 border-b border-[#2f3348] px-4 py-2">
        <div>
          <div className="text-[18px] font-semibold tabular-nums text-[#c0caf5]">{stats.sessionCount.toLocaleString()}</div>
          <div className="text-[10px] text-[#565f89]">sessions{filtersActive ? " (filtered)" : ""}</div>
        </div>
        <button onClick={() => onSortKey("events")} className="text-left hover:opacity-80" title="Sort by most events">
          <div className="text-[18px] font-semibold tabular-nums text-[#7aa2f7]">{stats.eventCount.toLocaleString()}</div>
          <div className="text-[10px] text-[#565f89]">events{sortKey === "events" ? " ↓" : ""}</div>
        </button>
        <button onClick={() => onSortKey("tokens")} className="text-left hover:opacity-80" title="Sort by most tokens">
          <div className="text-[18px] font-semibold tabular-nums text-[#e0af68]">{kfmt(stats.tokens)}</div>
          <div className="text-[10px] text-[#565f89]">tokens{sortKey === "tokens" ? " ↓" : ""}</div>
        </button>
        <div className="ml-auto flex min-w-0 flex-wrap items-center gap-4">
          <CopyLinkButton />
          {stats.busiest && (
            <button onClick={() => onOpenSession(stats.busiest!.session_id)} className="text-right hover:opacity-80">
              <div className="text-[11px] text-[#565f89]">busiest session</div>
              <div className="max-w-[280px] truncate text-[12px] text-[#c0caf5]">
                {sessionTitle(stats.busiest)} · <span className="text-[#7aa2f7]">{(stats.busiest.event_count ?? 0).toLocaleString()} ev</span>
              </div>
            </button>
          )}
        </div>
      </div>

      {/* Calendar — always the full universe so the shape of your history stays visible. */}
      <div className="overflow-x-auto border-b border-[#2f3348] px-4 py-3">
        <SessionCalendar sessions={[...universe]} selectedDay={selectedDay} onSelectDay={onSelectDay} />
      </div>

      {/* Recent strip — frecency where the eye already is. */}
      {recentSessions.length > 0 && (
        <div className="flex items-center gap-2 overflow-x-auto border-b border-[#2f3348] px-3 py-1.5" data-testid="recent-strip">
          <span className="shrink-0 text-[10px] uppercase tracking-wide text-[#565f89]">Recent</span>
          {recentSessions.map((s) => (
            <button
              key={s.session_id}
              data-recent-session={s.session_id}
              onClick={() => onOpenSession(s.session_id)}
              className="flex shrink-0 items-center gap-1.5 rounded-full border border-[#2f3348] px-2 py-0.5 text-[11px] text-[#c0caf5] hover:border-[#7aa2f7] hover:bg-[#24283b]"
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
          <div className="text-[13px] text-[#c0caf5]">No sessions match these filters</div>
          <div className="text-[11px] text-[#565f89]">
            {universe.length.toLocaleString()} sessions · try widening your search
          </div>
          {filtersActive && (
            <button
              onClick={onClearFilters}
              className="mt-1 rounded border border-[#7aa2f7] px-3 py-1 text-[11px] text-[#7aa2f7] hover:bg-[#7aa2f7]/10"
            >
              Reset filters
            </button>
          )}
        </div>
      ) : (
        <div className="flex flex-1 items-center justify-center p-8 text-center text-[#565f89]">
          <div>
            <div className="mb-2 text-lg">Select a session</div>
            <div className="text-xs">Pick one from the sidebar — filters and the calendar narrow the list.</div>
          </div>
        </div>
      )}
    </div>
  );
}
