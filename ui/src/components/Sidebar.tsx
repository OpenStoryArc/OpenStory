/**
 * Sidebar — session picker + agent navigation.
 *
 * Shows all sessions, and when one is selected, lists the agent spawns
 * within it. Clicking an agent zooms the timeline to that agent's subtree.
 */

import { useMemo, useState, useCallback, useRef, useEffect, memo } from "react";
import type { ViewRecord } from "@/types/view-record";
import type { WireRecord } from "@/types/wire-record";
import { compactTime } from "@/lib/time";
import { sampleDepthProfile } from "@/lib/depth-profile";
import { DepthSparkline } from "@/components/DepthSparkline";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface SessionInfo {
  id: string;
  eventCount: number;
  latestTimestamp: string;
  /** Events from the main agent (agent_id === null) */
  mainAgentCount: number;
  /** Subagents spawned in this session */
  subagents: SubagentInfo[];
  /** Human-readable label (first user prompt). */
  label: string | null;
  /** Git branch name. */
  branch: string | null;
  /** Downsampled depth profile for sparkline. */
  depthProfile: number[];
  /** Total tokens (input + output) for this session. */
  totalTokens: number;
  /** Number of completed plans (ExitPlanMode tool_calls). */
  planCount: number;
}

interface SubagentInfo {
  agentId: string;
  eventCount: number;
  firstTimestamp: string;
  /** A representative event ID for this agent (first seen) */
  representativeId: string;
  /** Delegation prompt description. */
  description: string | null;
}

import type { SessionLabel } from "@/types/websocket";

interface SidebarProps {
  events: readonly (ViewRecord | WireRecord)[];
  selectedSession: string | null;
  onSelectSession: (sessionId: string | null) => void;
  focusAgentId: string | null;
  onFocusAgent: (agentId: string | null) => void;
  sessionLabels?: Readonly<Record<string, SessionLabel>>;
  agentLabels?: Readonly<Record<string, string>>;
}

// ---------------------------------------------------------------------------
// Data derivation (pure functions)
// ---------------------------------------------------------------------------

export function deriveSessions(
  events: readonly (ViewRecord | WireRecord)[],
  sessionLabels?: Readonly<Record<string, SessionLabel>>,
  agentLabels?: Readonly<Record<string, string>>,
): SessionInfo[] {
  const sessionMap = new Map<string, {
    count: number;
    mainCount: number;
    planCount: number;
    latest: string;
    subagents: Map<string, { count: number; first: string; repId: string }>;
    depths: number[];
  }>();

  for (const ev of events) {
    const sid = ev.session_id;
    let session = sessionMap.get(sid);
    if (!session) {
      session = { count: 0, mainCount: 0, planCount: 0, latest: "", subagents: new Map(), depths: [] };
      sessionMap.set(sid, session);
    }
    session.count++;
    // Count completed plans (ExitPlanMode tool_calls)
    if (
      ev.record_type === "tool_call" &&
      ev.payload &&
      typeof ev.payload === "object" &&
      "name" in ev.payload &&
      (ev.payload as { name: string }).name === "ExitPlanMode"
    ) {
      session.planCount++;
    }
    if (ev.timestamp > session.latest) session.latest = ev.timestamp;
    if ("depth" in ev && typeof ev.depth === "number") {
      session.depths.push(ev.depth);
    }

    // Separate main agent vs subagent events
    const agentId = ev.agent_id;
    if (agentId) {
      let sub = session.subagents.get(agentId);
      if (!sub) {
        sub = { count: 0, first: ev.timestamp, repId: ev.id };
        session.subagents.set(agentId, sub);
      }
      sub.count++;
      if (ev.timestamp < sub.first) {
        sub.first = ev.timestamp;
        sub.repId = ev.id;
      }
    } else {
      session.mainCount++;
    }
  }

  const sessions: SessionInfo[] = [];
  for (const [id, data] of sessionMap) {
    const subagents: SubagentInfo[] = [];
    for (const [agentId, subData] of data.subagents) {
      subagents.push({
        agentId,
        eventCount: subData.count,
        firstTimestamp: subData.first,
        representativeId: subData.repId,
        description: agentLabels?.[agentId] ?? agentLabels?.[subData.repId] ?? null,
      });
    }
    subagents.sort((a, b) => a.firstTimestamp.localeCompare(b.firstTimestamp));
    const labelData = sessionLabels?.[id];
    sessions.push({
      id,
      eventCount: data.count,
      latestTimestamp: data.latest,
      mainAgentCount: data.mainCount,
      subagents,
      label: labelData?.label ?? null,
      branch: labelData?.branch ?? null,
      depthProfile: sampleDepthProfile(data.depths),
      totalTokens: (labelData?.total_input_tokens ?? 0) + (labelData?.total_output_tokens ?? 0),
      planCount: data.planCount,
    });
  }

  sessions.sort((a, b) => b.latestTimestamp.localeCompare(a.latestTimestamp));
  return sessions;
}

/** Format a token count as compact string: 1234 → "1.2K", 1234567 → "1.2M" */
function formatTokenCount(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

// ---------------------------------------------------------------------------
// Session colors
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

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const MIN_WIDTH = 180;
const MAX_WIDTH = 480;
const DEFAULT_WIDTH = 256;

export const Sidebar = memo(function Sidebar({
  events,
  selectedSession,
  onSelectSession,
  focusAgentId,
  onFocusAgent,
  sessionLabels,
  agentLabels,
}: SidebarProps) {
  const sessions = useMemo(
    () => deriveSessions(events, sessionLabels, agentLabels),
    [events, sessionLabels, agentLabels],
  );
  const selectedInfo = useMemo(
    () => sessions.find((s) => s.id === selectedSession) ?? null,
    [sessions, selectedSession],
  );

  // Keyboard navigation: up/down through sessions, right to timeline, enter to select
  const [highlightedIndex, setHighlightedIndex] = useState<number | null>(null);
  const [sidebarFocused, setSidebarFocused] = useState(false);
  const highlightedRef = useRef(highlightedIndex);
  highlightedRef.current = highlightedIndex;
  const sessionListRef = useRef<HTMLDivElement>(null);

  // Clear highlight when sessions change
  useEffect(() => { setHighlightedIndex(null); }, [sessions]);

  useEffect(() => {
    const el = sessionListRef.current;
    if (!el) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight") {
        e.preventDefault();
        const timeline = document.querySelector<HTMLElement>('[data-focus-zone="timeline"]');
        timeline?.focus();
        return;
      }
      if (e.key === "Enter" && highlightedRef.current !== null) {
        e.preventDefault();
        const s = sessions[highlightedRef.current];
        if (s) {
          onSelectSession(s.id);
          onFocusAgent(null);
        }
        return;
      }
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
      e.preventDefault();
      const current = highlightedRef.current;
      let next: number;
      if (current === null) {
        next = e.key === "ArrowDown" ? 0 : sessions.length - 1;
      } else {
        next = e.key === "ArrowDown"
          ? Math.min(current + 1, sessions.length - 1)
          : Math.max(current - 1, 0);
      }
      setHighlightedIndex(next);
      // Scroll highlighted session into view
      requestAnimationFrame(() => {
        const btn = el.querySelector(`[data-sidebar-index="${next}"]`);
        btn?.scrollIntoView({ block: "nearest" });
      });
    };

    el.addEventListener("keydown", onKeyDown);
    return () => el.removeEventListener("keydown", onKeyDown);
  }, [sessions, onSelectSession, onFocusAgent]);

  // --- Horizontal resize (sidebar width) ---
  const [width, setWidth] = useState(DEFAULT_WIDTH);
  const hDrag = useRef<{ active: boolean; startX: number; startW: number }>({ active: false, startX: 0, startW: 0 });

  const onHDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    hDrag.current = { active: true, startX: e.clientX, startW: width };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, [width]);

  // --- Vertical resize (sessions/agents split) ---
  // agentPct = percentage of sidebar height allocated to the agent panel
  const [agentPct, setAgentPct] = useState(35);
  const sidebarRef = useRef<HTMLDivElement>(null);
  const vDrag = useRef<{ active: boolean; startY: number; startPct: number }>({ active: false, startY: 0, startPct: 0 });

  const onVDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    vDrag.current = { active: true, startY: e.clientY, startPct: agentPct };
    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";
  }, [agentPct]);

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (hDrag.current.active) {
        const delta = e.clientX - hDrag.current.startX;
        setWidth(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, hDrag.current.startW + delta)));
      }
      if (vDrag.current.active && sidebarRef.current) {
        const sidebarH = sidebarRef.current.getBoundingClientRect().height;
        if (sidebarH > 0) {
          const deltaY = vDrag.current.startY - e.clientY; // dragging up = more agent space
          const deltaPct = (deltaY / sidebarH) * 100;
          setAgentPct(Math.min(70, Math.max(15, vDrag.current.startPct + deltaPct)));
        }
      }
    };
    const onMouseUp = () => {
      if (hDrag.current.active) {
        hDrag.current.active = false;
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      }
      if (vDrag.current.active) {
        vDrag.current.active = false;
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      }
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, []);

  return (
    <div
      ref={sidebarRef}
      data-testid="sidebar"
      className="flex flex-col bg-[#1a1b26] border-r border-[#2f3348] overflow-hidden relative"
      style={{ width, minWidth: MIN_WIDTH, maxWidth: MAX_WIDTH }}
    >
      {/* Width resize handle (right edge) */}
      <div
        onMouseDown={onHDragStart}
        className="absolute top-0 right-0 w-1 h-full cursor-col-resize hover:bg-[#7aa2f7] transition-colors z-10"
      />
      {/* Sessions header */}
      <div className="px-3 py-2 text-xs text-[#565f89] uppercase tracking-wider border-b border-[#2f3348] flex items-center justify-between">
        <span>Sessions</span>
        <span className="text-[#7aa2f7]" data-testid="sidebar-session-count">{sessions.length}</span>
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto min-h-0 outline-none" ref={sessionListRef} tabIndex={0} data-focus-zone="sidebar" onFocus={() => setSidebarFocused(true)} onBlur={() => setSidebarFocused(false)}>
        {sessions.map((s, i) => {
          const color = sessionColor(s.id);
          const isSelected = s.id === selectedSession;
          const isHighlighted = highlightedIndex === i;
          return (
            <button
              key={s.id}
              data-testid={`session-${s.id.slice(0, 8)}`}
              data-sidebar-index={i}
              onClick={() => {
                if (!isSelected) {
                  onSelectSession(s.id);
                  onFocusAgent(null);
                }
                setHighlightedIndex(i);
                sessionListRef.current?.focus();
              }}
              className={`w-full text-left px-3 py-2 border-b border-[#2f3348] transition-colors relative ${
                isSelected
                  ? "bg-[#24283b] border-l-2"
                  : "hover:bg-[#1e2030] cursor-pointer"
              }${sidebarFocused && isHighlighted ? " ring-1 ring-inset ring-[#7aa2f7]" : ""}`}
              style={isSelected ? { borderLeftColor: color } : undefined}
            >
              {/* Deselect button (only on selected session) */}
              {isSelected && (
                <span
                  data-testid="session-deselect"
                  role="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onSelectSession(null);
                    onFocusAgent(null);
                  }}
                  className="absolute top-1.5 right-2 text-[#565f89] hover:text-[#f7768e] text-xs cursor-pointer z-10"
                  title="Deselect session"
                >
                  ×
                </span>
              )}
              {s.label ? (
                <>
                  <div className="text-[11px] text-[#c0caf5] truncate leading-tight pr-4">
                    {s.label}
                  </div>
                  <div className="flex items-center gap-1.5 mt-0.5">
                    <span
                      className="text-[9px] px-1 py-0.5 rounded shrink-0"
                      style={{ color, backgroundColor: `${color}20` }}
                    >
                      {s.id.slice(0, 8)}
                    </span>
                    {s.branch && (
                      <span className="text-[9px] text-[#7dcfff] truncate">{s.branch}</span>
                    )}
                    <span className="text-[9px] text-[#565f89]">
                      {s.eventCount} · {compactTime(s.latestTimestamp)}
                    </span>
                    {s.totalTokens > 0 && (
                      <span className="text-[9px] text-[#e0af68]" title="Total tokens used">
                        {formatTokenCount(s.totalTokens)}
                      </span>
                    )}
                    {s.subagents.length > 0 && (
                      <span className="text-[9px] text-[#bb9af7]">
                        +{s.subagents.length}
                      </span>
                    )}
                    {s.planCount > 0 && (
                      <span className="text-[9px] text-[#9ece6a]" title={`${s.planCount} plan${s.planCount !== 1 ? "s" : ""}`}>
                        {s.planCount} plan{s.planCount !== 1 ? "s" : ""}
                      </span>
                    )}
                  </div>
                  <DepthSparkline profile={s.depthProfile} color={color} height={16} />
                </>
              ) : (
                <>
                  <div className="flex items-center gap-2">
                    <span
                      className="text-[10px] px-1.5 py-0.5 rounded shrink-0"
                      style={{ color, backgroundColor: `${color}20` }}
                    >
                      {s.id.slice(0, 8)}
                    </span>
                    <span className="text-[10px] text-[#565f89]">
                      {s.eventCount} events
                    </span>
                  </div>
                  <div className="text-[10px] text-[#565f89] mt-0.5">
                    {compactTime(s.latestTimestamp)}
                    {s.subagents.length > 0 && (
                      <span className="ml-2 text-[#bb9af7]">
                        {s.subagents.length} subagent{s.subagents.length !== 1 ? "s" : ""}
                      </span>
                    )}
                    {s.planCount > 0 && (
                      <span className="ml-2 text-[#9ece6a]">
                        {s.planCount} plan{s.planCount !== 1 ? "s" : ""}
                      </span>
                    )}
                  </div>
                  <DepthSparkline profile={s.depthProfile} color={color} height={16} />
                </>
              )}
            </button>
          );
        })}
      </div>

      {/* Agent hierarchy (when session selected) */}
      {selectedInfo && (
        <>
          {/* Vertical drag handle */}
          <div
            onMouseDown={onVDragStart}
            className="h-1 cursor-row-resize hover:bg-[#7aa2f7] transition-colors shrink-0 border-t border-[#2f3348]"
          />

          <div className="px-3 py-2 text-xs text-[#565f89] uppercase tracking-wider border-b border-[#2f3348] flex items-center justify-between shrink-0" data-testid="sidebar-agents-header">
            <span>Agents</span>
            <span className="text-[#bb9af7]">
              1{selectedInfo.subagents.length > 0 ? ` + ${selectedInfo.subagents.length}` : ""}
            </span>
          </div>

          <div className="overflow-y-auto" style={{ height: `${agentPct}%` }}>
            {/* "All events" option */}
            <button
              data-testid="agent-all"
              onClick={() => onFocusAgent(null)}
              className={`w-full text-left px-3 py-1.5 text-xs border-b border-[#2f3348] transition-colors ${
                !focusAgentId ? "bg-[#24283b] text-[#7aa2f7]" : "text-[#565f89] hover:bg-[#1e2030]"
              }`}
            >
              All events ({selectedInfo.eventCount})
            </button>

            {/* Main agent — always present */}
            <button
              data-testid="agent-main"
              onClick={() => onFocusAgent("__main__")}
              className={`w-full text-left px-3 py-1.5 border-b border-[#2f3348] transition-colors ${
                focusAgentId === "__main__"
                  ? "bg-[#24283b] border-l-2 border-l-[#7aa2f7]"
                  : "hover:bg-[#1e2030]"
              }`}
            >
              <div className="flex items-center gap-1.5">
                <span className="text-[11px] text-[#7aa2f7]">Main agent</span>
                <span className="text-[10px] text-[#565f89]">
                  {selectedInfo.mainAgentCount} events
                </span>
              </div>
            </button>

            {/* Subagents — indented under main */}
            {selectedInfo.subagents.map((sub) => {
              const isActive = focusAgentId === sub.agentId;
              return (
                <button
                  key={sub.agentId}
                  data-testid={`agent-${sub.agentId.slice(0, 16)}`}
                  onClick={() => onFocusAgent(isActive ? null : sub.agentId)}
                  className={`w-full text-left pl-6 pr-3 py-1.5 border-b border-[#2f3348] transition-colors ${
                    isActive
                      ? "bg-[#1a1a3e] border-l-2 border-l-[#bb9af7]"
                      : "hover:bg-[#1e2030]"
                  }`}
                >
                  <div className="flex items-center gap-1.5">
                    <span className="text-[10px] text-[#565f89]">└</span>
                    <span className="text-[11px] text-[#bb9af7] truncate">
                      {sub.description ?? sub.agentId.slice(0, 16)}
                    </span>
                  </div>
                  <div className="text-[10px] text-[#565f89] pl-4">
                    {sub.eventCount} events · {compactTime(sub.firstTimestamp)}
                  </div>
                </button>
              );
            })}

            {selectedInfo.subagents.length === 0 && (
              <div className="px-3 py-2 text-[10px] text-[#565f89] italic">
                No subagents spawned
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
});
