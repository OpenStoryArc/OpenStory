/** Session list sidebar for the Explore tab.
 *  Groups agent sessions under their parent. Defaults to non-stale. */

import { useState, useEffect, useMemo } from "react";
import type { SessionSummary } from "@/types/session";
import {
  filterSessionsByQuery,
  filterSessionsByStatus,
  filterSessionsByProject,
  extractProjects,
  computeStatusCounts,
  buildSessionHierarchy,
  type SessionStatusFilter,
  type ParentSession,
} from "@/lib/explore";
import { compactTime, formatDuration } from "@/lib/time";

// ---------------------------------------------------------------------------
// Session colors — same palette + hash as Live sidebar
// ---------------------------------------------------------------------------

const SESSION_COLORS = [
  "#7aa2f7", "#bb9af7", "#2ac3de", "#9ece6a", "#e0af68",
  "#f7768e", "#7dcfff", "#ff9e64", "#c0caf5", "#73daca",
];

function sessionColor(sessionId: string): string {
  let hash = 0;
  for (let i = 0; i < sessionId.length; i++) {
    hash = ((hash << 5) - hash + sessionId.charCodeAt(i)) | 0;
  }
  return SESSION_COLORS[Math.abs(hash) % SESSION_COLORS.length]!;
}

const STATUS_COLORS: Record<string, string> = {
  ongoing: "#9ece6a",
  completed: "#7aa2f7",
  errored: "#f7768e",
  stale: "#565f89",
};

// ---------------------------------------------------------------------------
// Props + filter config
// ---------------------------------------------------------------------------

interface ExploreSidebarProps {
  selectedSessionId: string | null;
  onSelectSession: (id: string) => void;
}

const STATUS_FILTERS: { key: SessionStatusFilter; label: string; color: string }[] = [
  { key: "all", label: "All", color: "#c0caf5" },
  { key: "ongoing", label: "Active", color: "#9ece6a" },
  { key: "completed", label: "Done", color: "#7aa2f7" },
  { key: "errored", label: "Errors", color: "#f7768e" },
  { key: "stale", label: "Stale", color: "#565f89" },
];

/** Default: show non-stale sessions. */
const DEFAULT_STATUS: SessionStatusFilter = "all";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function ExploreSidebar({ selectedSessionId, onSelectSession }: ExploreSidebarProps) {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<SessionStatusFilter>(DEFAULT_STATUS);
  const [projectFilter, setProjectFilter] = useState("");
  const [expandedParents, setExpandedParents] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    fetch("/api/sessions")
      .then((r) => r.json())
      .then((data: { sessions: SessionSummary[]; total: number }) => {
        if (!cancelled) {
          setSessions(data.sessions);
          setLoading(false);
        }
      })
      .catch(() => {
        if (!cancelled) setLoading(false);
      });
    return () => { cancelled = true; };
  }, []);

  // Compute counts from main sessions only (agents are nested, shouldn't inflate counts)
  const mainSessions = useMemo(
    () => sessions.filter((s) => !s.session_id.startsWith("agent-")),
    [sessions],
  );
  const statusCounts = useMemo(() => computeStatusCounts(mainSessions), [mainSessions]);
  const projects = useMemo(() => extractProjects(mainSessions), [mainSessions]);

  // Apply filters to all sessions, then build hierarchy
  const hierarchy = useMemo(() => {
    let filtered = filterSessionsByStatus(sessions, statusFilter);
    filtered = filterSessionsByProject(filtered, projectFilter);
    filtered = filterSessionsByQuery(filtered, query);
    return buildSessionHierarchy(filtered);
  }, [sessions, statusFilter, projectFilter, query]);

  const toggleExpand = (parentId: string) => {
    setExpandedParents((prev) => {
      const next = new Set(prev);
      if (next.has(parentId)) next.delete(parentId);
      else next.add(parentId);
      return next;
    });
  };

  return (
    <div className="w-64 shrink-0 flex flex-col border-r border-[#2f3348] bg-[#1a1b26] overflow-hidden" data-testid="explore-sidebar">
      {/* Header */}
      <div className="px-3 py-2 text-xs text-[#565f89] uppercase tracking-wider border-b border-[#2f3348] flex items-center justify-between">
        <span>Sessions</span>
        <span className="text-[#7aa2f7]">{hierarchy.length}</span>
      </div>

      {/* Search */}
      <div className="px-2 py-1.5 border-b border-[#2f3348]">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search..."
          className="w-full bg-[#24283b] text-[#c0caf5] text-[11px] rounded px-2 py-1 border border-[#2f3348] focus:border-[#7aa2f7] focus:outline-none placeholder-[#565f89]"
          data-testid="explore-search"
        />
      </div>

      {/* Status filter chips */}
      <div className="flex items-center gap-0.5 px-2 py-1 border-b border-[#2f3348]">
        {STATUS_FILTERS.map(({ key, label, color }) => {
          const count = statusCounts[key];
          return (
            <button
              key={key}
              onClick={() => setStatusFilter(key)}
              data-testid={`status-filter-${key}`}
              className={`px-1.5 py-0.5 rounded text-[10px] transition-colors ${
                statusFilter === key
                  ? "font-medium"
                  : "opacity-50 hover:opacity-80"
              }`}
              style={{
                color: statusFilter === key ? "#1a1b26" : color,
                backgroundColor: statusFilter === key ? color : `${color}10`,
              }}
            >
              {label}
              {count > 0 && key !== "all" && (
                <span className="ml-0.5 opacity-70">{count}</span>
              )}
            </button>
          );
        })}
      </div>

      {/* Project filter */}
      {projects.length > 1 && (
        <div className="px-2 py-1 border-b border-[#2f3348]">
          <select
            value={projectFilter}
            onChange={(e) => setProjectFilter(e.target.value)}
            className="w-full bg-[#24283b] text-[#c0caf5] text-[10px] rounded px-1.5 py-0.5 border border-[#2f3348] focus:border-[#7aa2f7] focus:outline-none"
            data-testid="project-filter"
          >
            <option value="">All projects</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} ({p.count})
              </option>
            ))}
          </select>
        </div>
      )}

      {/* Session list */}
      <div className="flex-1 overflow-y-auto min-h-0">
        {loading ? (
          <div className="p-3 text-xs text-[#565f89]">Loading sessions...</div>
        ) : hierarchy.length === 0 ? (
          <div className="p-3 text-xs text-[#565f89]">
            {query || statusFilter !== "all" || projectFilter
              ? "No sessions match filters"
              : "No sessions found"}
          </div>
        ) : (
          hierarchy.map((parent) => (
            <ParentCard
              key={parent.session.session_id}
              parent={parent}
              isSelected={selectedSessionId === parent.session.session_id}
              isExpanded={expandedParents.has(parent.session.session_id)}
              selectedSessionId={selectedSessionId}
              onSelect={onSelectSession}
              onToggleExpand={() => toggleExpand(parent.session.session_id)}
            />
          ))
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Parent session card with expandable agent list
// ---------------------------------------------------------------------------

function ParentCard({ parent, isSelected, isExpanded, selectedSessionId, onSelect, onToggleExpand }: {
  parent: ParentSession;
  isSelected: boolean;
  isExpanded: boolean;
  selectedSessionId: string | null;
  onSelect: (id: string) => void;
  onToggleExpand: () => void;
}) {
  const s = parent.session;
  const color = sessionColor(s.session_id);
  const statusColor = STATUS_COLORS[s.status] ?? "#565f89";
  const hasAgents = parent.agents.length > 0;

  return (
    <div className="border-b border-[#2f3348]">
      {/* Parent session */}
      <button
        onClick={() => onSelect(s.session_id)}
        data-testid={`explore-session-${s.session_id}`}
        className={`w-full text-left px-3 py-2 transition-colors ${
          isSelected
            ? "bg-[#24283b] border-l-2"
            : "hover:bg-[#1e2030] border-l-2 border-l-transparent"
        }`}
        style={isSelected ? { borderLeftColor: color } : undefined}
      >
        {/* Label / prompt */}
        {s.first_prompt && (
          <div className="text-[11px] text-[#c0caf5] truncate leading-tight mb-0.5">
            {s.first_prompt.length > 60 ? s.first_prompt.slice(0, 60) + "..." : s.first_prompt}
          </div>
        )}

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
            title={s.status}
          />
          <span className="text-[9px] text-[#565f89]">{s.event_count}</span>
          {s.duration_ms != null && s.duration_ms > 0 && (
            <span className="text-[9px] text-[#565f89]">{formatDuration(s.duration_ms)}</span>
          )}
          <span className="text-[9px] text-[#565f89] ml-auto">{compactTime(s.start_time)}</span>
        </div>

        {/* Project + agent count */}
        <div className="flex items-center gap-2 mt-0.5">
          {s.project_name && (
            <span className="text-[9px] text-[#7dcfff] truncate">{s.project_name}</span>
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
                    className={`w-full text-left pl-6 pr-3 py-1.5 text-xs transition-colors ${
                      agentSelected
                        ? "bg-[#24283b] border-l-2"
                        : "hover:bg-[#24283b] border-l-2 border-l-transparent"
                    }`}
                    style={agentSelected ? { borderLeftColor: agentColor } : undefined}
                  >
                    <div className="text-[10px] text-[#a9b1d6] truncate">
                      {a.first_prompt
                        ? a.first_prompt.length > 45
                          ? a.first_prompt.slice(0, 45) + "..."
                          : a.first_prompt
                        : a.session_id.slice(0, 16)}
                    </div>
                    <div className="flex items-center gap-1.5 mt-0.5">
                      <span
                        className="text-[8px] px-0.5 rounded"
                        style={{ color: agentColor, backgroundColor: `${agentColor}15` }}
                      >
                        {a.session_id.slice(6, 14)}
                      </span>
                      <span className="text-[9px] text-[#565f89]">{a.event_count}</span>
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
