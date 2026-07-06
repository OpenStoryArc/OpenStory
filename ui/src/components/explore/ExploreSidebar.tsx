/** Session list sidebar for the merged Explore tab.
 *
 *  Controlled component: ONE URL-owned filter model (OverviewFilters + sort)
 *  lives in ExploreView; this renders search, sort chips, date-range chips,
 *  facet groups, and the parent/subagent hierarchy list, and reports every
 *  change upward. Groups agent sessions under their parent.
 */

import { useMemo, useRef, useState } from "react";
import type { StorySession } from "@/lib/story-api";
import { buildSessionHierarchy, type ParentSession } from "@/lib/explore";
import {
  applyFilters,
  computeFacets,
  hasActiveFilters,
  sortSessions,
  SORT_LABELS,
  type OverviewFilters,
  type SortKey,
} from "@/lib/sessions-overview";
import { FacetGroup } from "./SessionFacets";
import { nextRowIndex } from "@/lib/keyboard-nav";
import { fullTimestamp, formatDuration } from "@/lib/time";
import { sessionTitle } from "@/lib/session-title";
import { sessionColor } from "@/lib/session-colors";
import { isSubagentSession } from "@/lib/subagents";
import { sessionDurationMs } from "@/lib/sessions-overview";
import { cn } from "@/lib/cn";

const STATUS_COLORS: Record<string, string> = {
  ongoing: "#9ece6a",
  completed: "#7aa2f7",
  errored: "#f7768e",
  stale: "#565f89",
};

const RANGE_CHIPS = [
  { key: "today", label: "Today" },
  { key: "7d", label: "7d" },
  { key: "30d", label: "30d" },
] as const;

interface ExploreSidebarProps {
  sessions: readonly StorySession[];
  loading: boolean;
  filters: OverviewFilters;
  sortKey: SortKey;
  /** Time anchor for the range chips — injectable so specs are deterministic. */
  nowMs?: number;
  onFiltersChange: (f: OverviewFilters) => void;
  onSortChange: (k: SortKey) => void;
  selectedSessionId: string | null;
  onSelectSession: (id: string) => void;
}

export function ExploreSidebar({
  sessions,
  loading,
  filters,
  sortKey,
  nowMs,
  onFiltersChange,
  onSortChange,
  selectedSessionId,
  onSelectSession,
}: ExploreSidebarProps) {
  const [expandedParents, setExpandedParents] = useState<Set<string>>(new Set());
  // Facets fold away by default — the sidebar's first job is the session list.
  const [facetsOpen, setFacetsOpen] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);

  // Facet counts come from the parent universe (agents are nested — they
  // shouldn't inflate counts), unfiltered so every choice stays visible.
  const universe = useMemo(
    () => sessions.filter((s) => !isSubagentSession(s.session_id)),
    [sessions],
  );
  const facets = useMemo(() => computeFacets(universe), [universe]);

  // Filter EVERYTHING (parents + agents) so children follow their parent,
  // sort parents by the chosen key, then fold into the hierarchy — which
  // preserves this order.
  const hierarchy = useMemo(() => {
    const filtered = applyFilters(sessions, filters, nowMs);
    return buildSessionHierarchy(sortSessions(filtered, sortKey));
  }, [sessions, filters, sortKey, nowMs]);

  const active = hasActiveFilters(filters);
  const activeFacetCount = (["project", "status", "agent", "user", "host", "branch"] as const)
    .filter((k) => filters[k]).length;
  const setFacet = (k: keyof Omit<OverviewFilters, "range">) => (val: string | undefined) =>
    onFiltersChange({ ...filters, [k]: val });

  const toggleExpand = (parentId: string) => {
    setExpandedParents((prev) => {
      const next = new Set(prev);
      if (next.has(parentId)) next.delete(parentId);
      else next.add(parentId);
      return next;
    });
  };

  // Keyboard nav over parent rows (j/k or arrows, Enter to open).
  const [highlight, setHighlight] = useState<number | null>(null);
  const onListKeyDown = (e: React.KeyboardEvent) => {
    const key = e.key;
    if (key === "ArrowDown" || key === "j" || key === "ArrowUp" || key === "k") {
      e.preventDefault();
      const dir = key === "ArrowDown" || key === "j" ? "down" : "up";
      setHighlight((h) => {
        const next = nextRowIndex(hierarchy.length, h, dir);
        if (next != null) {
          requestAnimationFrame(() => {
            const el = listRef.current?.querySelectorAll("[data-session-row]")[next] as HTMLElement | undefined;
            el?.scrollIntoView?.({ block: "nearest" });
          });
        }
        return next;
      });
    } else if (key === "Enter" && highlight != null) {
      e.preventDefault();
      const p = hierarchy[highlight];
      if (p) onSelectSession(p.session.session_id);
    }
  };

  return (
    <div className="w-72 shrink-0 flex flex-col border-r border-[#2f3348] bg-[#1a1b26] overflow-hidden" data-testid="explore-sidebar">
      {/* Header */}
      <div className="px-3 py-2 text-xs text-[#565f89] uppercase tracking-wider border-b border-[#2f3348] flex items-center justify-between">
        <span>Sessions</span>
        <span className="text-[#7aa2f7]">{hierarchy.length}</span>
      </div>

      {/* Search */}
      <div className="px-2 py-1.5 border-b border-[#2f3348]">
        <input
          type="text"
          value={filters.search ?? ""}
          onChange={(e) => onFiltersChange({ ...filters, search: e.target.value || undefined })}
          placeholder="Search..."
          className="w-full bg-[#24283b] text-[#c0caf5] text-[11px] rounded px-2 py-1 border border-[#2f3348] focus:border-[#7aa2f7] focus:outline-none placeholder-[#565f89]"
          data-testid="explore-search"
        />
      </div>

      {/* Sort chips */}
      <div className="flex flex-wrap items-center gap-1 px-2 py-1 border-b border-[#2f3348]">
        {(Object.keys(SORT_LABELS) as SortKey[]).map((k) => (
          <button
            key={k}
            onClick={() => onSortChange(k)}
            data-testid={`sort-${k}`}
            className={cn(
              "rounded px-1.5 py-0.5 text-[10px] transition-colors",
              sortKey === k ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#565f89] hover:bg-[#2f3348] hover:text-[#c0caf5]",
            )}
          >
            {SORT_LABELS[k]}
          </button>
        ))}
      </div>

      {/* Date-range chips — bookmarkable via filters.range */}
      <div className="flex items-center gap-1 px-2 py-1 border-b border-[#2f3348] text-[10px]">
        {RANGE_CHIPS.map((r) => (
          <button
            key={r.key}
            onClick={() => onFiltersChange({ ...filters, range: filters.range === r.key ? undefined : r.key, day: undefined })}
            data-testid={`date-range-${r.key}`}
            className={cn(
              "px-1.5 py-0.5 rounded transition-colors",
              filters.range === r.key ? "bg-[#7aa2f7] text-[#1a1b26] font-medium" : "text-[#565f89] hover:text-[#c0caf5]",
            )}
          >
            {r.label}
          </button>
        ))}
        <button
          onClick={() => onFiltersChange({ ...filters, range: undefined })}
          data-testid="date-range-all"
          className={cn(
            "px-1.5 py-0.5 rounded transition-colors",
            !filters.range ? "bg-[#7aa2f7] text-[#1a1b26] font-medium" : "text-[#565f89] hover:text-[#c0caf5]",
          )}
        >
          All
        </button>
      </div>

      {/* Facets + active-filter controls */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="border-b border-[#2f3348]">
          <div className="p-2 pb-0">
            {active && (
              <button
                onClick={() => onFiltersChange({})}
                className="mb-2 w-full rounded border border-[#f7768e]/40 px-2 py-0.5 text-[11px] text-[#f7768e] hover:bg-[#f7768e]/10"
              >
                Clear all filters
              </button>
            )}
            {filters.day && (
              <button
                onClick={() => onFiltersChange({ ...filters, day: undefined })}
                className="mb-2 flex w-full items-center justify-between rounded bg-[#7aa2f7] px-2 py-0.5 text-[11px] text-[#1a1b26]"
              >
                <span>📅 {filters.day}</span>
                <span>✕</span>
              </button>
            )}
          </div>
          <button
            onClick={() => setFacetsOpen((o) => !o)}
            data-testid="facets-toggle"
            className="flex w-full items-center gap-1 px-3 pb-1.5 pt-0.5 text-[10px] uppercase tracking-wide text-[#565f89] hover:text-[#c0caf5]"
          >
            <span>{facetsOpen ? "▾" : "▸"}</span>
            <span>Filters</span>
            {activeFacetCount > 0 && (
              <span className="rounded-full bg-[#7aa2f7] px-1.5 text-[9px] font-medium text-[#1a1b26]">{activeFacetCount}</span>
            )}
          </button>
          {facetsOpen && (
            <div className="px-2 pb-2">
              <FacetGroup group="project" title="Project" values={facets.projects} selected={filters.project} onSelect={setFacet("project")} />
              <FacetGroup group="status" title="Status" values={facets.statuses} selected={filters.status} onSelect={setFacet("status")} />
              <FacetGroup group="agent" title="Agent" values={facets.agents} selected={filters.agent} onSelect={setFacet("agent")} />
              <FacetGroup group="user" title="User" values={facets.users} selected={filters.user} onSelect={setFacet("user")} />
              <FacetGroup group="host" title="Host" values={facets.hosts} selected={filters.host} onSelect={setFacet("host")} />
              <FacetGroup group="branch" title="Branch" values={facets.branches} selected={filters.branch} onSelect={setFacet("branch")} />
            </div>
          )}
        </div>

        {/* Session list */}
        <div
          ref={listRef}
          tabIndex={0}
          onKeyDown={onListKeyDown}
          aria-label="Session list (j/k to navigate, Enter to open)"
          className="outline-none"
        >
          {loading ? (
            <div className="p-3 text-xs text-[#565f89]">Loading sessions...</div>
          ) : hierarchy.length === 0 ? (
            <div className="p-3 text-xs text-[#565f89]">
              {active ? "No sessions match filters" : "No sessions found"}
            </div>
          ) : (
            hierarchy.map((parent, i) => (
              <ParentCard
                key={parent.session.session_id}
                parent={parent}
                isSelected={selectedSessionId === parent.session.session_id}
                isHighlighted={highlight === i}
                isExpanded={expandedParents.has(parent.session.session_id)}
                selectedSessionId={selectedSessionId}
                onSelect={onSelectSession}
                onToggleExpand={() => toggleExpand(parent.session.session_id)}
              />
            ))
          )}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Parent session card with expandable agent list
// ---------------------------------------------------------------------------

function ParentCard({ parent, isSelected, isHighlighted, isExpanded, selectedSessionId, onSelect, onToggleExpand }: {
  parent: ParentSession<StorySession>;
  isSelected: boolean;
  isHighlighted: boolean;
  isExpanded: boolean;
  selectedSessionId: string | null;
  onSelect: (id: string) => void;
  onToggleExpand: () => void;
}) {
  const s = parent.session;
  const color = sessionColor(s.session_id);
  const statusColor = STATUS_COLORS[s.status ?? ""] ?? "#565f89";
  const hasAgents = parent.agents.length > 0;
  const durMs = sessionDurationMs(s);

  return (
    <div className="border-b border-[#2f3348]">
      {/* Parent session */}
      <button
        onClick={() => onSelect(s.session_id)}
        data-testid={`explore-session-${s.session_id}`}
        data-session-row={s.session_id}
        className={cn(
          "w-full text-left px-3 py-2 transition-colors border-l-2",
          isSelected ? "bg-[#24283b]" : "hover:bg-[#1e2030] border-l-transparent",
          isHighlighted && "ring-1 ring-inset ring-[#7aa2f7]",
        )}
        style={isSelected ? { borderLeftColor: color } : undefined}
      >
        {/* Human title — hidden when it would just repeat the id chip below. */}
        {(() => {
          const title = sessionTitle(s);
          return title !== s.session_id.slice(0, 8) ? (
            <div className="text-[11px] text-[#c0caf5] truncate leading-tight mb-0.5" title={title}>{title}</div>
          ) : null;
        })()}

        {/* Metadata row */}
        <div className="flex items-center gap-1.5 flex-wrap">
          <span
            className="text-[9px] px-1 py-0.5 rounded shrink-0"
            style={{ color, backgroundColor: `${color}20` }}
          >
            {s.session_id.slice(0, 8)}
          </span>
          <span
            className="w-1.5 h-1.5 rounded-full shrink-0"
            style={{ backgroundColor: statusColor }}
            title={s.status ?? "unknown"}
          />
          <span className="text-[9px] text-[#565f89]">{s.event_count ?? 0}</span>
          {durMs > 0 && (
            <span className="text-[9px] text-[#565f89]">{formatDuration(durMs)}</span>
          )}
          <span className="text-[9px] text-[#565f89] ml-auto tabular-nums" title="latest activity">
            {fullTimestamp(s.last_event ?? s.start_time ?? "")}
          </span>
        </div>

        {/* Project + agent count */}
        <div className="flex items-center gap-2 mt-0.5">
          {s.project_name && (
            <span className="text-[9px] text-[#7dcfff] truncate" title={s.project_name}>{s.project_name}</span>
          )}
          {hasAgents && (
            <span className="text-[9px] text-[#bb9af7] ml-auto shrink-0">
              {parent.agents.length} agent{parent.agents.length !== 1 ? "s" : ""}
            </span>
          )}
        </div>
      </button>

      {/* Agent expand toggle */}
      {hasAgents && (
        <>
          <button
            onClick={onToggleExpand}
            className="w-full px-3 py-1 text-[10px] text-[#bb9af7] hover:bg-[#1e2030] transition-colors flex items-center gap-1"
          >
            <span>{isExpanded ? "▾" : "▸"}</span>
            <span>{parent.agents.length} subagent{parent.agents.length !== 1 ? "s" : ""}</span>
            <span className="text-[#565f89]">({parent.totalAgentEvents} events)</span>
          </button>

          {/* Agent list */}
          {isExpanded && (
            <div className="bg-[#1e2030]">
              {parent.agents.map((a) => {
                const agentColor = sessionColor(a.session_id);
                const agentSelected = selectedSessionId === a.session_id;
                return (
                  <button
                    key={a.session_id}
                    onClick={() => onSelect(a.session_id)}
                    className={cn(
                      "w-full text-left pl-6 pr-3 py-1.5 text-xs transition-colors border-l-2",
                      agentSelected ? "bg-[#24283b]" : "hover:bg-[#24283b] border-l-transparent",
                    )}
                    style={agentSelected ? { borderLeftColor: agentColor } : undefined}
                  >
                    <div className="text-[10px] text-[#a9b1d6] truncate" title={sessionTitle(a)}>
                      {sessionTitle(a)}
                    </div>
                    <div className="flex items-center gap-1.5 mt-0.5">
                      <span
                        className="text-[8px] px-0.5 rounded"
                        style={{ color: agentColor, backgroundColor: `${agentColor}15` }}
                      >
                        {a.session_id.slice(6, 14)}
                      </span>
                      <span className="text-[9px] text-[#565f89]">{a.event_count ?? 0}</span>
                    </div>
                  </button>
                );
              })}
            </div>
          )}
        </>
      )}
    </div>
  );
}
