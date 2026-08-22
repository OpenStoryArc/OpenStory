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
import { originAgentColor, originAgentLabel } from "@/lib/origin-agent";

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
    <div className="w-72 shrink-0 flex flex-col border-r border-[color:var(--divider)] bg-[color:var(--bg)] overflow-hidden" data-testid="explore-sidebar">
      {/* Header */}
      <div className="px-3 py-2 text-xs text-[color:var(--text-muted)] uppercase tracking-wider border-b border-[color:var(--divider)] flex items-center justify-between">
        <span>Sessions</span>
        <span className="text-[color:var(--accent)]">{hierarchy.length}</span>
      </div>

      {/* Search */}
      <div className="px-2 py-1.5 border-b border-[color:var(--divider)]">
        <input
          type="text"
          value={filters.search ?? ""}
          onChange={(e) => onFiltersChange({ ...filters, search: e.target.value || undefined })}
          placeholder="Search..."
          className="w-full bg-[color:var(--bg-surface)] text-[color:var(--text)] text-[11px] rounded px-2 py-1 border border-[color:var(--divider)] focus:border-[color:var(--accent)] focus:outline-none placeholder-[color:var(--text-muted)]"
          data-testid="explore-search"
        />
      </div>

      {/* Sort chips */}
      <div className="flex flex-wrap items-center gap-1 px-2 py-1 border-b border-[color:var(--divider)]">
        {(Object.keys(SORT_LABELS) as SortKey[]).map((k) => (
          <button
            key={k}
            onClick={() => onSortChange(k)}
            data-testid={`sort-${k}`}
            className={cn(
              "rounded px-1.5 py-0.5 text-[10px] transition-colors",
              sortKey === k ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:bg-[color:var(--bg-hover)] hover:text-[color:var(--text)]",
            )}
          >
            {SORT_LABELS[k]}
          </button>
        ))}
      </div>

      {/* Date-range chips — bookmarkable via filters.range */}
      <div className="flex items-center gap-1 px-2 py-1 border-b border-[color:var(--divider)] text-[10px]">
        {RANGE_CHIPS.map((r) => (
          <button
            key={r.key}
            onClick={() => onFiltersChange({ ...filters, range: filters.range === r.key ? undefined : r.key, day: undefined })}
            data-testid={`date-range-${r.key}`}
            className={cn(
              "px-1.5 py-0.5 rounded transition-colors",
              filters.range === r.key ? "bg-[color:var(--accent)] text-[color:var(--bg)] font-medium" : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]",
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
            !filters.range ? "bg-[color:var(--accent)] text-[color:var(--bg)] font-medium" : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]",
          )}
        >
          All
        </button>
      </div>

      {/* Facets + active-filter controls */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="border-b border-[color:var(--divider)]">
          <div className="p-2 pb-0">
            {active && (
              <button
                onClick={() => onFiltersChange({})}
                className="mb-2 w-full rounded border border-[color:var(--red)]/40 px-2 py-0.5 text-[11px] text-[color:var(--red)] hover:bg-[color:var(--red)]/10"
              >
                Clear all filters
              </button>
            )}
            {filters.day && (
              <button
                onClick={() => onFiltersChange({ ...filters, day: undefined })}
                className="mb-2 flex w-full items-center justify-between rounded bg-[color:var(--accent)] px-2 py-0.5 text-[11px] text-[color:var(--bg)]"
              >
                <span>📅 {filters.day}</span>
                <span>✕</span>
              </button>
            )}
          </div>
          <button
            onClick={() => setFacetsOpen((o) => !o)}
            data-testid="facets-toggle"
            className="flex w-full items-center gap-1 px-3 pb-1.5 pt-0.5 text-[10px] uppercase tracking-wide text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
          >
            <span>{facetsOpen ? "▾" : "▸"}</span>
            <span>Filters</span>
            {activeFacetCount > 0 && (
              <span className="rounded-full bg-[color:var(--accent)] px-1.5 text-[9px] font-medium text-[color:var(--bg)]">{activeFacetCount}</span>
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
            <div className="p-3 text-xs text-[color:var(--text-muted)]">Loading sessions...</div>
          ) : hierarchy.length === 0 ? (
            <div className="p-3 text-xs text-[color:var(--text-muted)]">
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
    <div className="border-b border-[color:var(--divider)]">
      {/* Parent session */}
      <button
        onClick={() => onSelect(s.session_id)}
        data-testid={`explore-session-${s.session_id}`}
        data-session-row={s.session_id}
        data-os-target="session"
        data-os-id={s.session_id}
        data-session-id={s.session_id}
        className={cn(
          "w-full text-left px-3 py-2 transition-colors border-l-2",
          isSelected ? "bg-[color:var(--bg-surface)]" : "hover:bg-[color:var(--bg-surface)] border-l-transparent",
          isHighlighted && "ring-1 ring-inset ring-[color:var(--accent)]",
        )}
        style={isSelected ? { borderLeftColor: color } : undefined}
      >
        {/* Human title — hidden when it would just repeat the id chip below. */}
        {(() => {
          const title = sessionTitle(s);
          return title !== s.session_id.slice(0, 8) ? (
            <div className="text-[11px] text-[color:var(--text)] truncate leading-tight mb-0.5" title={title}>{title}</div>
          ) : null;
        })()}

        {/* Metadata row */}
        <div className="flex items-center gap-1.5 flex-wrap">
          {(() => {
            const agent = (s as { origin_agent?: string | null }).origin_agent;
            const label = originAgentLabel(agent);
            if (!label) return null;
            const ac = originAgentColor(agent);
            return (
              <span
                className="text-[9px] font-medium px-1 py-0.5 rounded shrink-0"
                style={{ color: ac, backgroundColor: `${ac}20` }}
                data-testid="session-card-agent-badge"
                title={`Agent: ${label}`}
              >
                {label}
              </span>
            );
          })()}
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
          <span className="text-[9px] text-[color:var(--text-muted)]">{s.event_count ?? 0}</span>
          {durMs > 0 && (
            <span className="text-[9px] text-[color:var(--text-muted)]">{formatDuration(durMs)}</span>
          )}
          <span className="text-[9px] text-[color:var(--text-muted)] ml-auto tabular-nums" title="latest activity">
            {fullTimestamp(s.last_event ?? s.start_time ?? "")}
          </span>
        </div>

        {/* Project + agent count */}
        <div className="flex items-center gap-2 mt-0.5">
          {s.project_name && (
            <span className="text-[9px] text-[color:var(--cyan-bright)] truncate" title={s.project_name}>{s.project_name}</span>
          )}
          {hasAgents && (
            <span className="text-[9px] text-[color:var(--purple)] ml-auto shrink-0">
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
            className="w-full px-3 py-1 text-[10px] text-[color:var(--purple)] hover:bg-[color:var(--bg-surface)] transition-colors flex items-center gap-1"
          >
            <span>{isExpanded ? "▾" : "▸"}</span>
            <span>{parent.agents.length} subagent{parent.agents.length !== 1 ? "s" : ""}</span>
            <span className="text-[color:var(--text-muted)]">({parent.totalAgentEvents} events)</span>
          </button>

          {/* Agent list */}
          {isExpanded && (
            <div className="bg-[color:var(--bg-surface)]">
              {parent.agents.map((a) => {
                const agentColor = sessionColor(a.session_id);
                const agentSelected = selectedSessionId === a.session_id;
                return (
                  <button
                    key={a.session_id}
                    onClick={() => onSelect(a.session_id)}
                    className={cn(
                      "w-full text-left pl-6 pr-3 py-1.5 text-xs transition-colors border-l-2",
                      agentSelected ? "bg-[color:var(--bg-surface)]" : "hover:bg-[color:var(--bg-surface)] border-l-transparent",
                    )}
                    style={agentSelected ? { borderLeftColor: agentColor } : undefined}
                  >
                    <div className="text-[10px] text-[color:var(--text-bright)] truncate" title={sessionTitle(a)}>
                      {sessionTitle(a)}
                    </div>
                    <div className="flex items-center gap-1.5 mt-0.5">
                      <span
                        className="text-[8px] px-0.5 rounded"
                        style={{ color: agentColor, backgroundColor: `${agentColor}15` }}
                      >
                        {a.session_id.slice(6, 14)}
                      </span>
                      <span className="text-[9px] text-[color:var(--text-muted)]">{a.event_count ?? 0}</span>
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
