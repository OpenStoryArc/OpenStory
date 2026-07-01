/** OverviewView — the Sessions dashboard.
 *
 *  A bird's-eye view of every session: a calendar heatmap of activity over
 *  time (click a day to filter), facet filters (project / host / user / branch
 *  / status / agent) + free-text search, a sortable list ("most events",
 *  "most tokens", "longest", "recent"), and a click-in drill-down that shows a
 *  session's activity ribbon + full stats summary.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { nextRowIndex } from "@/lib/keyboard-nav";
import type { StorySession } from "@/lib/story-api";
import { buildHash, type HashRoute, type OverviewRoute } from "@/lib/hash-route";
import {
  applyFilters,
  computeFacets,
  computeStats,
  hasActiveFilters,
  pickRecentSessions,
  projectKey,
  sessionDurationMs,
  sessionTokens,
  sortSessions,
  SORT_LABELS,
  type FacetValue,
  type OverviewFilters,
  type SortKey,
} from "@/lib/sessions-overview";
import { useRecents } from "@/hooks/use-recents";
import { isSubagentSession } from "@/lib/subagents";
import { SessionCalendar } from "@/components/viz/SessionCalendar";
import { SessionVizLoader } from "@/components/viz/SessionVizLoader";
import { SessionDetailPanel } from "@/components/session/SessionDetailPanel";
import { SessionListSkeleton } from "./OverviewSkeletons";
import { sessionColor } from "@/lib/session-colors";
import { formatDuration, relativeTime } from "@/lib/time";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { cn } from "@/lib/cn";

interface Props {
  route: HashRoute;
  onNavigate: (route: HashRoute) => void;
}

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

function sessionTitle(s: StorySession): string {
  const raw = (s.label || s.first_prompt || "").trim();
  if (!raw) return s.session_id.slice(0, 8);
  // Humanize harness-wrapper labels (e.g. "/loop …") so the list stays readable.
  return cleanHarnessPreview(raw).trim() || s.session_id.slice(0, 8);
}

// ── filter sidebar ──────────────────────────────────────────────────────────

function FacetGroup({
  title,
  values,
  selected,
  onSelect,
  color,
}: {
  title: string;
  values: FacetValue[];
  selected: string | undefined;
  onSelect: (key: string | undefined) => void;
  color?: (key: string) => string;
}) {
  const [showAll, setShowAll] = useState(false);
  if (values.length === 0) return null;
  const shown = showAll ? values : values.slice(0, 8);
  return (
    <div className="mb-3">
      <div className="mb-1 px-1 text-[10px] font-medium uppercase tracking-wide text-[#565f89]">{title}</div>
      <div className="flex flex-col gap-0.5">
        {shown.map((v) => {
          const active = selected === v.key;
          return (
            <button
              key={v.key}
              onClick={() => onSelect(active ? undefined : v.key)}
              className={cn(
                "flex items-center justify-between rounded px-2 py-0.5 text-left text-[11px] transition-colors",
                active ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#c0caf5] hover:bg-[#2f3348]",
              )}
            >
              <span className="flex items-center gap-1.5 truncate">
                {color && <span className="inline-block h-2 w-2 shrink-0 rounded-full" style={{ background: color(v.key) }} />}
                <span className="truncate">{v.key}</span>
              </span>
              <span className={cn("ml-2 shrink-0 tabular-nums", active ? "text-[#1a1b26]" : "text-[#565f89]")}>{v.count}</span>
            </button>
          );
        })}
        {values.length > 8 && (
          <button onClick={() => setShowAll((s) => !s)} className="px-2 py-0.5 text-left text-[10px] text-[#7aa2f7] hover:text-[#89b4fa]">
            {showAll ? "Show less" : `+${values.length - 8} more`}
          </button>
        )}
      </div>
    </div>
  );
}

// ── session row ─────────────────────────────────────────────────────────────

function SessionRow({
  s,
  selected,
  highlighted,
  isBusiest,
  onClick,
}: {
  s: StorySession;
  selected: boolean;
  highlighted: boolean;
  isBusiest: boolean;
  onClick: () => void;
}) {
  const color = sessionColor(s.session_id);
  const tokens = sessionTokens(s);
  const dur = sessionDurationMs(s);
  return (
    <button
      onClick={onClick}
      data-session-row={s.session_id}
      data-highlighted={highlighted ? "true" : undefined}
      className={cn(
        "flex w-full items-center gap-3 border-b border-[#2f3348]/60 px-3 py-2 text-left transition-colors",
        selected ? "bg-[#2f3348]" : "hover:bg-[#24283b]",
        highlighted && "ring-1 ring-inset ring-[#7aa2f7]",
      )}
    >
      <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: color }} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[12px] text-[#c0caf5]">{sessionTitle(s)}</span>
          {isBusiest && (
            <span className="shrink-0 rounded bg-[#e0af68]/20 px-1 text-[9px] font-medium text-[#e0af68]">busiest</span>
          )}
          {s.status === "ongoing" && (
            <span className="shrink-0 rounded bg-[#9ece6a]/20 px-1 text-[9px] font-medium text-[#9ece6a]">live</span>
          )}
        </div>
        <div className="flex items-center gap-2 text-[10px] text-[#565f89]">
          <span className="truncate" style={{ color: `${color}bb` }}>{projectKey(s)}</span>
          {s.branch && <span className="truncate">· {s.branch}</span>}
          {s.user && <span>· {s.user}{s.host ? `@${s.host}` : ""}</span>}
        </div>
      </div>
      <div className="shrink-0 text-right text-[10px] tabular-nums text-[#565f89]">
        <div>
          <span className="text-[#7aa2f7]">{(s.event_count ?? 0).toLocaleString()}</span> ev
          {tokens > 0 && <span className="ml-2 text-[#e0af68]">{kfmt(tokens)}</span>} {tokens > 0 && "tok"}
        </div>
        <div>
          {dur > 0 && <span>{formatDuration(dur)}</span>}
          {s.last_event && <span className="ml-2">{relativeTime(s.last_event)}</span>}
        </div>
      </div>
    </button>
  );
}

// ── drill-in panel ──────────────────────────────────────────────────────────

function DrillIn({ sessionId, onClose, onOpenExplore, onOpenStory, onOpenSubagent }: { sessionId: string; onClose: () => void; onOpenExplore: () => void; onOpenStory: () => void; onOpenSubagent: (id: string) => void }) {
  return (
    <aside className="flex w-[420px] shrink-0 flex-col border-l border-[#2f3348] bg-[#1a1b26]">
      <div className="flex items-center justify-between border-b border-[#2f3348] px-3 py-2">
        <span className="truncate font-mono text-[11px] text-[#565f89]">{sessionId.slice(0, 12)}…</span>
        <div className="flex items-center gap-2">
          <button onClick={onOpenStory} className="rounded px-2 py-0.5 text-[11px] text-[#bb9af7] hover:bg-[#2f3348]">Story →</button>
          <button onClick={onOpenExplore} className="rounded px-2 py-0.5 text-[11px] text-[#7aa2f7] hover:bg-[#2f3348]">
            Explore →
          </button>
          <button onClick={onClose} className="rounded px-1.5 text-[#565f89] hover:text-[#c0caf5]" aria-label="Close">✕</button>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="border-b border-[#2f3348]">
          <SessionVizLoader sessionId={sessionId} onOpenSubagent={onOpenSubagent} />
        </div>
        <SessionDetailPanel sessionId={sessionId} />
      </div>
    </aside>
  );
}

// ── main view ───────────────────────────────────────────────────────────────

export function OverviewView({ route, onNavigate }: Props) {
  const { sessions, loading, error, refresh } = useSessionsList();
  // Hydrate initial state from the URL so a pasted/bookmarked link restores the
  // exact filtered view. (Read once on mount; local state is the interactive
  // truth thereafter, mirrored back into the URL below.)
  const [filters, setFilters] = useState<OverviewFilters>(() => route.overview?.filters ?? {});
  const [sortKey, setSortKey] = useState<SortKey>(() => route.overview?.sort ?? "recent");
  const [selectedId, setSelectedId] = useState<string | null>(() => route.overview?.sessionId ?? null);

  // Mirror state → URL (replaceState, so it stays copyable without spamming
  // browser history on every keystroke or filter toggle).
  useEffect(() => {
    const overview: OverviewRoute = { filters };
    if (sortKey !== "recent") overview.sort = sortKey;
    if (selectedId) overview.sessionId = selectedId;
    const hash = buildHash({ view: "overview", overview });
    if (hash !== window.location.hash) {
      window.history.replaceState(null, "", hash);
    }
  }, [filters, sortKey, selectedId]);

  const { recentIds, record } = useRecents();
  // Subagents are stored as separate agent-* sessions (~44% of the list). Hide
  // them from the top-level universe by default; a toggle brings them back.
  const [showSubagents, setShowSubagents] = useState(false);
  const subagentCount = useMemo(() => sessions.filter((s) => isSubagentSession(s.session_id)).length, [sessions]);
  const universe = useMemo(
    () => (showSubagents ? sessions : sessions.filter((s) => !isSubagentSession(s.session_id))),
    [sessions, showSubagents],
  );

  const facets = useMemo(() => computeFacets(universe), [universe]);
  const filtered = useMemo(() => applyFilters(universe, filters), [universe, filters]);
  const sorted = useMemo(() => sortSessions(filtered, sortKey), [filtered, sortKey]);
  const stats = useMemo(() => computeStats(filtered), [filtered]);
  const busiestId = stats.busiest?.session_id;
  const recentSessions = useMemo(() => pickRecentSessions(universe, recentIds, 5), [universe, recentIds]);

  // Open a session in the drill-in and remember the visit (feeds frecency).
  const openSession = (id: string) => {
    setSelectedId(id);
    record(id);
  };

  // Keyboard navigation for the session list (j/k or arrows, Enter to open).
  const [highlight, setHighlight] = useState<number | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => { setHighlight(null); }, [sorted]);
  const onListKeyDown = (e: React.KeyboardEvent) => {
    const key = e.key;
    if (key === "ArrowDown" || key === "j") {
      e.preventDefault();
      setHighlight((h) => {
        const next = nextRowIndex(sorted.length, h, "down");
        if (next != null) scrollRowIntoView(next);
        return next;
      });
    } else if (key === "ArrowUp" || key === "k") {
      e.preventDefault();
      setHighlight((h) => {
        const next = nextRowIndex(sorted.length, h, "up");
        if (next != null) scrollRowIntoView(next);
        return next;
      });
    } else if (key === "Enter" && highlight != null) {
      e.preventDefault();
      const s = sorted[highlight];
      if (s) openSession(s.session_id);
    }
  };
  const scrollRowIntoView = (i: number) => {
    requestAnimationFrame(() => {
      const el = listRef.current?.querySelectorAll("[data-session-row]")[i] as HTMLElement | undefined;
      el?.scrollIntoView?.({ block: "nearest" });
    });
  };

  const setFacet = (k: keyof OverviewFilters) => (val: string | undefined) =>
    setFilters((f) => ({ ...f, [k]: val }));

  const active = hasActiveFilters(filters);

  return (
    <div className="flex min-h-0 flex-1 bg-[#1a1b26] text-[#c0caf5]">
      {/* Filters sidebar */}
      <div className="flex w-56 shrink-0 flex-col border-r border-[#2f3348] bg-[#1a1b26]">
        <div className="border-b border-[#2f3348] p-2">
          <input
            value={filters.search ?? ""}
            onChange={(e) => setFilters((f) => ({ ...f, search: e.target.value }))}
            placeholder="Search sessions…"
            className="w-full rounded border border-[#2f3348] bg-[#24283b] px-2 py-1 text-[12px] text-[#c0caf5] placeholder:text-[#565f89] focus:border-[#7aa2f7] focus:outline-none"
          />
          <div className="mt-2 flex flex-wrap gap-1">
            {(Object.keys(SORT_LABELS) as SortKey[]).map((k) => (
              <button
                key={k}
                onClick={() => setSortKey(k)}
                className={cn(
                  "rounded px-1.5 py-0.5 text-[10px] transition-colors",
                  sortKey === k ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#565f89] hover:bg-[#2f3348] hover:text-[#c0caf5]",
                )}
              >
                {SORT_LABELS[k]}
              </button>
            ))}
          </div>
          {subagentCount > 0 && (
            <button
              onClick={() => setShowSubagents((v) => !v)}
              className="mt-2 flex w-full items-center gap-1.5 text-[10px] text-[#565f89] hover:text-[#c0caf5]"
              title="Subagents are stored as separate agent-* sessions"
            >
              <span className={cn("inline-block h-3 w-3 rounded-sm border", showSubagents ? "border-[#7aa2f7] bg-[#7aa2f7]" : "border-[#3b4261]")} />
              Show subagents ({subagentCount.toLocaleString()})
            </button>
          )}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {active && (
            <button
              onClick={() => setFilters({})}
              className="mb-2 w-full rounded border border-[#f7768e]/40 px-2 py-0.5 text-[11px] text-[#f7768e] hover:bg-[#f7768e]/10"
            >
              Clear all filters
            </button>
          )}
          {filters.day && (
            <button
              onClick={() => setFacet("day")(undefined)}
              className="mb-2 flex w-full items-center justify-between rounded bg-[#7aa2f7] px-2 py-0.5 text-[11px] text-[#1a1b26]"
            >
              <span>📅 {filters.day}</span>
              <span>✕</span>
            </button>
          )}
          <FacetGroup title="Project" values={facets.projects} selected={filters.project} onSelect={setFacet("project")} />
          <FacetGroup title="Status" values={facets.statuses} selected={filters.status} onSelect={setFacet("status")} />
          <FacetGroup title="Agent" values={facets.agents} selected={filters.agent} onSelect={setFacet("agent")} />
          <FacetGroup title="User" values={facets.users} selected={filters.user} onSelect={setFacet("user")} />
          <FacetGroup title="Host" values={facets.hosts} selected={filters.host} onSelect={setFacet("host")} />
          <FacetGroup title="Branch" values={facets.branches} selected={filters.branch} onSelect={setFacet("branch")} />
        </div>
      </div>

      {/* Main column */}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* Stats bar */}
        <div className="flex items-center gap-6 border-b border-[#2f3348] px-4 py-2">
          <div>
            <div className="text-[18px] font-semibold tabular-nums text-[#c0caf5]">{stats.sessionCount.toLocaleString()}</div>
            <div className="text-[10px] text-[#565f89]">sessions{active ? " (filtered)" : ""}</div>
          </div>
          <button onClick={() => setSortKey("events")} className="text-left hover:opacity-80" title="Sort by most events">
            <div className="text-[18px] font-semibold tabular-nums text-[#7aa2f7]">{stats.eventCount.toLocaleString()}</div>
            <div className="text-[10px] text-[#565f89]">events{sortKey === "events" ? " ↓" : ""}</div>
          </button>
          <button onClick={() => setSortKey("tokens")} className="text-left hover:opacity-80" title="Sort by most tokens">
            <div className="text-[18px] font-semibold tabular-nums text-[#e0af68]">{kfmt(stats.tokens)}</div>
            <div className="text-[10px] text-[#565f89]">tokens{sortKey === "tokens" ? " ↓" : ""}</div>
          </button>
          <div className="ml-auto flex items-center gap-4">
            <CopyLinkButton />
            {stats.busiest && (
              <button
                onClick={() => openSession(stats.busiest!.session_id)}
                className="text-right hover:opacity-80"
              >
                <div className="text-[11px] text-[#565f89]">busiest session</div>
                <div className="max-w-[280px] truncate text-[12px] text-[#c0caf5]">
                  {sessionTitle(stats.busiest)} · <span className="text-[#7aa2f7]">{(stats.busiest.event_count ?? 0).toLocaleString()} ev</span>
                </div>
              </button>
            )}
          </div>
        </div>

        {/* Calendar (always over the full, unfiltered universe) */}
        <div className="overflow-x-auto border-b border-[#2f3348] px-4 py-3">
          <SessionCalendar
            sessions={universe}
            selectedDay={filters.day ?? null}
            onSelectDay={(day) => setFilters((f) => ({ ...f, day: day ?? undefined }))}
          />
        </div>

        {/* Session list + drill-in */}
        <div className="flex min-h-0 flex-1">
          <div
            ref={listRef}
            className="min-w-0 flex-1 overflow-y-auto outline-none"
            tabIndex={0}
            onKeyDown={onListKeyDown}
            aria-label="Session list (j/k to navigate, Enter to open)"
          >
            {/* Recent strip — frecency where the eye already is */}
            {!loading && recentSessions.length > 0 && !active && (
              <div className="flex items-center gap-2 overflow-x-auto border-b border-[#2f3348] bg-[#1a1b26] px-3 py-1.5" data-testid="recent-strip">
                <span className="shrink-0 text-[10px] uppercase tracking-wide text-[#565f89]">Recent</span>
                {recentSessions.map((s) => (
                  <button
                    key={s.session_id}
                    data-recent-session={s.session_id}
                    onClick={() => openSession(s.session_id)}
                    className="flex shrink-0 items-center gap-1.5 rounded-full border border-[#2f3348] px-2 py-0.5 text-[11px] text-[#c0caf5] hover:border-[#7aa2f7] hover:bg-[#24283b]"
                    title={sessionTitle(s)}
                  >
                    <span className="h-2 w-2 rounded-full" style={{ background: sessionColor(s.session_id) }} />
                    <span className="max-w-[160px] truncate">{sessionTitle(s)}</span>
                  </button>
                ))}
              </div>
            )}
            {loading ? (
              <SessionListSkeleton />
            ) : error && sessions.length === 0 ? (
              <div className="flex flex-col items-center gap-2 p-8 text-center" data-testid="overview-error">
                <div className="text-[13px] text-[#f7768e]">Couldn't load sessions</div>
                <div className="max-w-[420px] break-words text-[11px] text-[#565f89]">{error}</div>
                <button
                  onClick={refresh}
                  className="mt-1 rounded border border-[#7aa2f7] px-3 py-1 text-[11px] text-[#7aa2f7] hover:bg-[#7aa2f7]/10"
                >
                  Retry
                </button>
              </div>
            ) : sorted.length === 0 ? (
              <div className="flex flex-col items-center gap-2 p-8 text-center">
                <div className="text-[13px] text-[#c0caf5]">No sessions match these filters</div>
                <div className="text-[11px] text-[#565f89]">
                  {universe.length.toLocaleString()} sessions · try widening your search
                </div>
                {active && (
                  <button
                    onClick={() => setFilters({})}
                    className="mt-1 rounded border border-[#7aa2f7] px-3 py-1 text-[11px] text-[#7aa2f7] hover:bg-[#7aa2f7]/10"
                  >
                    Reset filters
                  </button>
                )}
              </div>
            ) : (
              sorted.map((s, i) => (
                <SessionRow
                  key={s.session_id}
                  s={s}
                  selected={selectedId === s.session_id}
                  highlighted={highlight === i}
                  isBusiest={s.session_id === busiestId}
                  onClick={() => openSession(s.session_id)}
                />
              ))
            )}
          </div>

          {selectedId && (
            <DrillIn
              sessionId={selectedId}
              onClose={() => setSelectedId(null)}
              onOpenExplore={() => onNavigate({ view: "explore", sessionId: selectedId })}
              onOpenStory={() => onNavigate({ view: "story", sessionId: selectedId })}
              onOpenSubagent={openSession}
            />
          )}
        </div>
      </div>
    </div>
  );
}
