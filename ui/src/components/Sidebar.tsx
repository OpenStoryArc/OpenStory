/**
 * Sidebar — session picker + agent navigation.
 *
 * Shows all sessions, and when one is selected, lists the agent spawns
 * within it. Clicking an agent zooms the timeline to that agent's subtree.
 */

import { useMemo, useState, useCallback, useRef, useEffect, memo } from "react";
import type { ViewRecord } from "@/types/view-record";
import type { WireRecord } from "@/types/wire-record";
import type { StorySession, Fleet } from "@/lib/story-api";
import { Timestamp } from "@/components/ui/Timestamp";
import { sampleDepthProfile } from "@/lib/depth-profile";
import { sessionColor } from "@/lib/session-colors";
import { DepthSparkline } from "@/components/DepthSparkline";
import { PersonRow } from "@/components/PersonRow";
import { TimeFilter } from "@/components/TimeFilter";
import { timeFilterMatches, TIME_FILTER_LABELS, type TimeFilterKey } from "@/lib/time-filter";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { originAgentColor, originAgentLabel } from "@/lib/origin-agent";
import { useFleet } from "@/hooks/use-fleet";

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
  /** Origin host (machine identity), `null` for legacy / unstamped sessions. */
  host: string | null;
  /** Origin user (human identity), `null` for legacy / unstamped sessions. */
  user: string | null;
  /** Agent platform that produced the session. */
  originAgent: string | null;
  /** OpenStory directory person id, `null` for legacy / unstamped sessions. */
  personId: string | null;
  /** Producing principal id (which device/agent in the fleet), `null` for legacy. */
  principalId: string | null;
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
  /** Active user filter (URL-driven). `null` = "All users". */
  userFilter?: string | null;
  /** Setter — typically wired to a `navigate({ view: "live", userFilter })` call. */
  onUserFilterChange?: (user: string | null) => void;
  /** Active time-window filter (URL-driven). Defaults to "all" when omitted. */
  timeFilter?: "1h" | "today" | "week" | "all";
  /** Setter — typically wired to a navigate({...timeFilter}) call. */
  onTimeFilterChange?: (next: "1h" | "today" | "week" | "all") => void;
}

// ---------------------------------------------------------------------------
// Data derivation (pure functions)
// ---------------------------------------------------------------------------

/** Derive the sidebar's session list.
 *
 *  Pre-redesign: the universe of sessions came from `events` (every
 *  WireRecord seen). After feat/lazy-load-initial-state, records arrive
 *  lazily on session-open, so `events` is empty for unloaded sessions
 *  and the universe must come from a REST `/api/sessions` snapshot.
 *
 *  The function merges both inputs:
 *    - Every entry in `restSessions` produces a `SessionInfo` row, even
 *      with no loaded records (rich indicators like subagent count and
 *      depthProfile show as zero/empty until that session is opened).
 *    - When `events` contains records for a session, those records
 *      override the REST baseline for eventCount, depthProfile, and the
 *      agent/plan aggregates.
 *
 *  Back-compat: `restSessions` is optional. Tests that only pass
 *  `events` continue to work — the derived list is whatever the records
 *  reveal, exactly as before.
 */
export function deriveSessions(
  events: readonly (ViewRecord | WireRecord)[],
  sessionLabels?: Readonly<Record<string, SessionLabel>>,
  restSessions?: readonly StorySession[],
): SessionInfo[] {
  const sessionMap = new Map<string, {
    count: number;
    mainCount: number;
    planCount: number;
    latest: string;
    originAgent: string | null;
    subagents: Map<string, { count: number; first: string; repId: string }>;
    depths: number[];
    /** True when this session was seeded from REST (events may still be empty). */
    fromRest: boolean;
  }>();

  if (restSessions) {
    for (const r of restSessions) {
      sessionMap.set(r.session_id, {
        count: r.event_count ?? 0,
        mainCount: 0,
        planCount: 0,
        latest: r.last_event ?? r.start_time ?? "",
        originAgent: r.origin_agent ?? null,
        subagents: new Map(),
        depths: [],
        fromRest: true,
      });
    }
  }

  // Records-derived data overrides the REST baseline for loaded sessions.
  // We track loaded sessions separately so `count` is taken from records,
  // not added to the REST baseline.
  const loadedFromEvents = new Set<string>();
  for (const ev of events) {
    const sid = ev.session_id;
    if (!loadedFromEvents.has(sid)) {
      loadedFromEvents.add(sid);
      const baseline = sessionMap.get(sid);
      // Reset counts to zero — records about to be re-tallied.
      sessionMap.set(sid, {
        count: 0,
        mainCount: 0,
        planCount: 0,
        latest: baseline?.latest ?? "",
        originAgent: baseline?.originAgent ?? null,
        subagents: new Map(),
        depths: [],
        fromRest: baseline?.fromRest ?? false,
      });
    }
    const session = sessionMap.get(sid)!;
    session.count++;
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
    if (!session.originAgent && ev.origin_agent) {
      session.originAgent = ev.origin_agent;
    }
    if ("depth" in ev && typeof ev.depth === "number") {
      session.depths.push(ev.depth);
    }

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
  // REST sessions carry their own label/branch/tokens; prefer the WS
  // sessionLabels when present (it carries live token deltas).
  const restById = new Map<string, StorySession>();
  if (restSessions) {
    for (const r of restSessions) restById.set(r.session_id, r);
  }
  for (const [id, data] of sessionMap) {
    const subagents: SubagentInfo[] = [];
    for (const [agentId, subData] of data.subagents) {
      subagents.push({
        agentId,
        eventCount: subData.count,
        firstTimestamp: subData.first,
        representativeId: subData.repId,
        description: null,
      });
    }
    subagents.sort((a, b) => a.firstTimestamp.localeCompare(b.firstTimestamp));

    const labelData = sessionLabels?.[id];
    const restRow = restById.get(id);
    const label = labelData?.label ?? restRow?.label ?? null;
    const branch = labelData?.branch ?? restRow?.branch ?? null;
    const totalTokens =
      (labelData?.total_input_tokens ?? restRow?.total_input_tokens ?? 0) +
      (labelData?.total_output_tokens ?? restRow?.total_output_tokens ?? 0);

    sessions.push({
      id,
      eventCount: data.count,
      latestTimestamp: data.latest,
      mainAgentCount: data.mainCount,
      subagents,
      label,
      branch,
      depthProfile: sampleDepthProfile(data.depths),
      totalTokens,
      // Durable plan_count (plan store) is authoritative; max with the
      // event-derived count so a live ExitPlanMode still bumps the badge
      // before the REST list refreshes.
      planCount: Math.max(restRow?.plan_count ?? 0, data.planCount),
      host: restRow?.host ?? null,
      user: restRow?.user ?? null,
      originAgent: data.originAgent ?? restRow?.origin_agent ?? null,
      personId: restRow?.person_id ?? null,
      principalId: restRow?.principal_id ?? null,
    });
  }

  sessions.sort((a, b) => b.latestTimestamp.localeCompare(a.latestTimestamp));
  return sessions;
}

/** A bucket of sessions that all belong to the same principal. */
export interface PrincipalGroup {
  /** `null` for sessions with no principal_id (legacy / unstamped). */
  readonly principalId: string | null;
  /** Friendly label — fleet's display_name for the principal, or
   *  "Unattributed" when principalId is `null`, or the principalId
   *  itself as a fallback when the fleet has no entry for it. */
  readonly principalName: string;
  readonly sessions: readonly SessionInfo[];
}

/**
 * Group sessions by principalId, resolving display names from the fleet.
 * Pure function — exported for BDD tests.
 *
 * Ordering: groups sort by their newest session's `latestTimestamp` (DESC),
 * with the "Unattributed" bucket pinned last so legacy data doesn't push
 * live fleet activity out of view.
 */
export function groupSessionsByPrincipal(
  sessions: readonly SessionInfo[],
  fleet: Fleet | null,
): PrincipalGroup[] {
  // Fleet → principalId → display_name lookup. Empty when fleet is null.
  const nameById = new Map<string, string>();
  if (fleet) {
    for (const p of fleet.principals) nameById.set(p.id, p.display_name);
  }

  // Bucket sessions by principalId; preserve incoming order within bucket
  // (sessions arrive sorted by latestTimestamp DESC from `deriveSessions`).
  const buckets = new Map<string | null, SessionInfo[]>();
  for (const s of sessions) {
    const key = s.principalId ?? null;
    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = [];
      buckets.set(key, bucket);
    }
    bucket.push(s);
  }

  const groups: PrincipalGroup[] = [];
  for (const [principalId, sessionsInGroup] of buckets) {
    groups.push({
      principalId,
      principalName:
        principalId === null
          ? "Unattributed"
          : (nameById.get(principalId) ?? principalId),
      sessions: sessionsInGroup,
    });
  }

  // Order groups by newest session (DESC). Unattributed always last.
  groups.sort((a, b) => {
    if (a.principalId === null && b.principalId === null) return 0;
    if (a.principalId === null) return 1;
    if (b.principalId === null) return -1;
    const aTs = a.sessions[0]?.latestTimestamp ?? "";
    const bTs = b.sessions[0]?.latestTimestamp ?? "";
    return bTs.localeCompare(aTs);
  });
  return groups;
}

/** Format a token count as compact string: 1234 → "1.2K", 1234567 → "1.2M" */
function formatTokenCount(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

// Session colors live in @/lib/session-colors (imported at top of file)
// so the Story tab can share the same deterministic palette.

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
  userFilter: userFilterProp,
  onUserFilterChange,
  timeFilter: timeFilterProp,
  onTimeFilterChange,
}: SidebarProps) {
  // The sidebar's universe of sessions comes from REST (/api/sessions)
  // since `initial_state` no longer ships records. Records for the
  // currently-open session arrive lazily and augment per-session
  // indicators (subagent count, depth profile, plan count).
  const { sessions: restSessions } = useSessionsList();
  const { fleet } = useFleet();
  const sessions = useMemo(
    () => deriveSessions(events, sessionLabels, restSessions),
    [events, sessionLabels, restSessions],
  );
  const selectedInfo = useMemo(
    () => sessions.find((s) => s.id === selectedSession) ?? null,
    [sessions, selectedSession],
  );

  // Origin filters: when set, narrow the visible session list to those
  // matching the host and/or user. Sessions with `null` origin (legacy,
  // unstamped) never match a non-null filter — same posture as the
  // server-side ?host= / ?user= query parameters: a filter shouldn't
  // leak rows whose origin we simply don't know.
  //
  // `userFilter` is URL-driven (passed in via props). `hostFilter` stays
  // local — it's a finer-grained narrowing on top of "which person", and
  // hasn't yet earned its place in the URL. Lift it later if needed.
  const [hostFilter, setHostFilter] = useState<string | null>(null);
  // Bridge URL-driven userFilter into a setter the existing chip-row
  // and PersonRow can call: when there's no parent setter, fall back
  // to local state (test scaffolding & callers that don't pass props).
  const [localUserFilter, setLocalUserFilter] = useState<string | null>(null);
  const userFilter = userFilterProp ?? localUserFilter;
  const setUserFilter = useCallback(
    (u: string | null) => {
      if (onUserFilterChange) onUserFilterChange(u);
      else setLocalUserFilter(u);
    },
    [onUserFilterChange],
  );
  // Same prop-or-local pattern for the time filter. Default is "all"
  // so the rendered TimeFilter has a sensible selection on first paint.
  const [localTimeFilter, setLocalTimeFilter] = useState<TimeFilterKey>("all");
  const timeFilter: TimeFilterKey = timeFilterProp ?? localTimeFilter;
  const setTimeFilter = useCallback(
    (next: TimeFilterKey) => {
      if (onTimeFilterChange) onTimeFilterChange(next);
      else setLocalTimeFilter(next);
    },
    [onTimeFilterChange],
  );
  const filteredSessions = useMemo(() => {
    if (!hostFilter && !userFilter && timeFilter === "all") return sessions;
    // Capture `now` once per render so all sessions are checked against
    // the same instant. Reading Date.now() inside the predicate would
    // make the boundary skew by a microsecond per session.
    const now = Date.now();
    return sessions.filter(
      (s) =>
        (!hostFilter || s.host === hostFilter) &&
        (!userFilter || s.user === userFilter) &&
        timeFilterMatches(s.latestTimestamp, timeFilter, now),
    );
  }, [sessions, hostFilter, userFilter, timeFilter]);

  // Group filtered sessions by principalId — the "your fleet" sidebar
  // shape. Each group's header carries the principal's display_name from
  // the fleet config; sessions render unchanged within their group.
  const principalGroups = useMemo(
    () => groupSessionsByPrincipal(filteredSessions, fleet),
    [filteredSessions, fleet],
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
        const s = filteredSessions[highlightedRef.current];
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
        next = e.key === "ArrowDown" ? 0 : filteredSessions.length - 1;
      } else {
        next = e.key === "ArrowDown"
          ? Math.min(current + 1, filteredSessions.length - 1)
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
  const [width, setWidth] = useState(() => {
    // Persisted like the other resizable panels — a chosen width survives reloads.
    if (typeof window === "undefined") return DEFAULT_WIDTH;
    const saved = Number(window.localStorage.getItem("live.sidebar.width"));
    return Number.isFinite(saved) && saved >= MIN_WIDTH && saved <= MAX_WIDTH ? saved : DEFAULT_WIDTH;
  });
  useEffect(() => {
    if (typeof window !== "undefined") window.localStorage.setItem("live.sidebar.width", String(width));
  }, [width]);
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
      className="flex flex-col bg-[color:var(--bg)] border-r border-[color:var(--bg-hover)] overflow-hidden relative"
      style={{ width, minWidth: MIN_WIDTH, maxWidth: MAX_WIDTH }}
    >
      {/* Width resize handle (right edge) */}
      <div
        onMouseDown={onHDragStart}
        className="absolute top-0 right-0 w-1 h-full cursor-col-resize hover:bg-[color:var(--accent)] transition-colors z-10"
      />
      {/* Person filter row — primary user-filter surface, hidden when 0/1 stamped users. */}
      <PersonRow userFilter={userFilter} onUserFilterChange={setUserFilter} />

      {/* Time-window filter — Last Hour / Today / This Week / All. */}
      <TimeFilter value={timeFilter} onChange={setTimeFilter} />

      {/* Sessions header */}
      <div className="px-3 py-2 text-xs text-[color:var(--text-muted)] uppercase tracking-wider border-b border-[color:var(--bg-hover)] flex items-center justify-between">
        <span>Sessions</span>
        <span className="text-[color:var(--accent)]" data-testid="sidebar-session-count">
          {hostFilter || userFilter || timeFilter !== "all"
            ? `${filteredSessions.length} / ${sessions.length}`
            : sessions.length}
        </span>
      </div>

      {/* Origin filter chips — only rendered when at least one filter is active. */}
      {(hostFilter || userFilter) && (
        <div className="px-3 py-1.5 border-b border-[color:var(--bg-hover)] flex items-center gap-1.5 flex-wrap" data-testid="origin-filter-chips">
          {hostFilter && (
            <button
              type="button"
              onClick={() => setHostFilter(null)}
              className="text-[length:var(--fs-label)] px-1.5 py-0.5 rounded bg-[color:var(--accent)]/13 text-[color:var(--accent)] hover:bg-[color:var(--accent)]/25 transition-colors flex items-center gap-1"
              title="Clear host filter"
              data-testid="filter-chip-host"
            >
              host: {hostFilter}
              <span className="text-[color:var(--text-muted)]">×</span>
            </button>
          )}
          {userFilter && (
            <button
              type="button"
              onClick={() => setUserFilter(null)}
              className="text-[length:var(--fs-label)] px-1.5 py-0.5 rounded bg-[color:var(--accent)]/13 text-[color:var(--accent)] hover:bg-[color:var(--accent)]/25 transition-colors flex items-center gap-1"
              title="Clear user filter"
              data-testid="filter-chip-user"
            >
              user: {userFilter}
              <span className="text-[color:var(--text-muted)]">×</span>
            </button>
          )}
        </div>
      )}

      {/* Session list */}
      <div className="flex-1 overflow-y-auto min-h-0 outline-none" ref={sessionListRef} tabIndex={0} data-focus-zone="sidebar" onFocus={() => setSidebarFocused(true)} onBlur={() => setSidebarFocused(false)}>
        {/* Empty state when a filter is active but nothing matches.
            Distinct from "store has no sessions at all" — that case
            wasn't previously handled either; for now we show this when
            sessions.length > 0 to keep the message accurate. */}
        {filteredSessions.length === 0 && sessions.length > 0 && (hostFilter || userFilter || timeFilter !== "all") && (
          <div className="px-4 py-6 text-center text-xs text-[color:var(--text-muted)]" data-testid="sidebar-no-matches">
            <div className="mb-1 text-[color:var(--accent)]">
              No sessions match{userFilter ? ` @${userFilter}` : ""}
              {hostFilter ? ` ⌂ ${hostFilter}` : ""}
              {timeFilter !== "all" ? ` · ${TIME_FILTER_LABELS[timeFilter]}` : ""}
            </div>
            <div className="text-[length:var(--fs-label)] mb-2">
              {sessions.length} stamped session{sessions.length === 1 ? "" : "s"} on this leaf,
              none from this filter yet.
            </div>
            <button
              type="button"
              onClick={() => {
                setHostFilter(null);
                setUserFilter(null);
                setTimeFilter("all");
              }}
              className="text-[length:var(--fs-label)] px-2 py-1 rounded bg-[color:var(--accent)]/13 text-[color:var(--accent)] hover:bg-[color:var(--accent)]/25 transition-colors"
              data-testid="sidebar-clear-filters"
            >
              Clear filters
            </button>
          </div>
        )}
        {principalGroups.map((group) => (
          <div
            key={`group-${group.principalId ?? "unattributed"}`}
            data-testid={`fleet-group-${group.principalId ?? "unattributed"}`}
          >
            <div className="px-3 py-1.5 text-[length:var(--fs-label)] font-semibold text-[color:var(--accent)] bg-[color:var(--bg)] border-b border-[color:var(--bg-hover)] uppercase tracking-wider sticky top-0 z-10 flex items-center justify-between">
              <span className="truncate" data-testid="fleet-group-name" title={group.principalName}>
                {group.principalName}
              </span>
              <span className="text-[color:var(--text-muted)] normal-case font-normal text-[length:var(--fs-label)]">
                {group.sessions.length}
              </span>
            </div>
            {group.sessions.map((s) => {
              const i = filteredSessions.indexOf(s);
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
              className={`w-full text-left px-3 py-2 border-b border-[color:var(--bg-hover)] transition-colors relative ${
                isSelected
                  ? "bg-[color:var(--bg-surface)] border-l-2"
                  : "hover:bg-[color:var(--bg-surface)] cursor-pointer"
              }${sidebarFocused && isHighlighted ? " ring-1 ring-inset ring-[color:var(--accent)]" : ""}`}
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
                  className="absolute top-1.5 right-2 text-[color:var(--text-muted)] hover:text-[color:var(--red)] text-xs cursor-pointer z-10"
                  title="Deselect session"
                >
                  ×
                </span>
              )}
              {s.label ? (
                <>
                  {(() => {
                    const agentLabel = originAgentLabel(s.originAgent);
                    const agentColor = originAgentColor(s.originAgent);
                    return agentLabel ? (
                      <div className="flex items-center mb-0.5">
                        <span
                          className="text-[length:var(--fs-label)] px-1 py-0.5 rounded"
                          style={{ color: agentColor, backgroundColor: `${agentColor}20` }}
                          title={`Agent: ${agentLabel}`}
                          data-testid="session-agent-badge"
                        >
                          {agentLabel}
                        </span>
                      </div>
                    ) : null;
                  })()}
                  <div className="text-[length:var(--fs-body)] text-[color:var(--text)] truncate leading-tight pr-4" title={s.label}>
                    {s.label}
                  </div>
                  <div className="flex items-center gap-1.5 mt-0.5">
                    <span
                      className="text-[length:var(--fs-label)] px-1 py-0.5 rounded shrink-0"
                      style={{ color, backgroundColor: `${color}20` }}
                    >
                      {s.id.slice(0, 8)}
                    </span>
                    {s.branch && (
                      <span className="text-[length:var(--fs-label)] text-[color:var(--cyan-bright)] truncate" title={s.branch}>{s.branch}</span>
                    )}
                    <span className="text-[length:var(--fs-label)] text-[color:var(--text-muted)]">
                      {s.eventCount} · <Timestamp iso={s.latestTimestamp} />
                    </span>
                    {s.totalTokens > 0 && (
                      <span className="text-[length:var(--fs-label)] text-[color:var(--orange)]" title="Total tokens used">
                        {formatTokenCount(s.totalTokens)}
                      </span>
                    )}
                    {s.subagents.length > 0 && (
                      <span className="text-[length:var(--fs-label)] text-[color:var(--purple)]">
                        +{s.subagents.length}
                      </span>
                    )}
                    {s.planCount > 0 && (
                      <span className="text-[length:var(--fs-label)] text-[color:var(--green)]" title={`${s.planCount} plan${s.planCount !== 1 ? "s" : ""}`}>
                        {s.planCount} plan{s.planCount !== 1 ? "s" : ""}
                      </span>
                    )}
                  </div>
                  {/* Origin badge — clickable to filter the sidebar.
                      Hidden entirely when both host and user are null
                      (legacy / unstamped sessions don't get a placeholder). */}
                  {(s.host || s.user) && (
                    <div className="flex items-center gap-1 mt-0.5" data-testid="session-origin-badge">
                      {s.host && (
                        <span
                          role="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            setHostFilter(s.host);
                          }}
                          className="text-[length:var(--fs-label)] text-[color:var(--accent)] hover:bg-[color:var(--accent)]/13 px-1 rounded cursor-pointer"
                          title={`Filter to host: ${s.host}`}
                        >
                          ⌂ {s.host}
                        </span>
                      )}
                      {s.user && (
                        <span
                          role="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            setUserFilter(s.user);
                          }}
                          className="text-[length:var(--fs-label)] text-[color:var(--accent)] hover:bg-[color:var(--accent)]/13 px-1 rounded cursor-pointer"
                          title={`Filter to user: ${s.user}`}
                        >
                          @{s.user}
                        </span>
                      )}
                    </div>
                  )}
                  <DepthSparkline profile={s.depthProfile} color={color} height={16} />
                </>
              ) : (
                <>
                  <div className="flex items-center gap-2">
                    {(() => {
                      const agentLabel = originAgentLabel(s.originAgent);
                      const agentColor = originAgentColor(s.originAgent);
                      return agentLabel ? (
                        <span
                          className="text-[length:var(--fs-label)] px-1 py-0.5 rounded shrink-0"
                          style={{ color: agentColor, backgroundColor: `${agentColor}20` }}
                          title={`Agent: ${agentLabel}`}
                          data-testid="session-agent-badge"
                        >
                          {agentLabel}
                        </span>
                      ) : null;
                    })()}
                    <span
                      className="text-[length:var(--fs-label)] px-1.5 py-0.5 rounded shrink-0"
                      style={{ color, backgroundColor: `${color}20` }}
                    >
                      {s.id.slice(0, 8)}
                    </span>
                    <span className="text-[length:var(--fs-label)] text-[color:var(--text-muted)]">
                      {s.eventCount} events
                    </span>
                  </div>
                  <div className="text-[length:var(--fs-label)] text-[color:var(--text-muted)] mt-0.5">
                    <Timestamp iso={s.latestTimestamp} />
                    {s.subagents.length > 0 && (
                      <span className="ml-2 text-[color:var(--purple)]">
                        {s.subagents.length} subagent{s.subagents.length !== 1 ? "s" : ""}
                      </span>
                    )}
                    {s.planCount > 0 && (
                      <span className="ml-2 text-[color:var(--green)]">
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
        ))}
      </div>

      {/* Agent hierarchy (when session selected) */}
      {selectedInfo && (
        <>
          {/* Vertical drag handle */}
          <div
            onMouseDown={onVDragStart}
            className="h-1 cursor-row-resize hover:bg-[color:var(--accent)] transition-colors shrink-0 border-t border-[color:var(--bg-hover)]"
          />

          <div className="px-3 py-2 text-xs text-[color:var(--text-muted)] uppercase tracking-wider border-b border-[color:var(--bg-hover)] flex items-center justify-between shrink-0" data-testid="sidebar-agents-header">
            <span>Agents</span>
            <span className="text-[color:var(--purple)]">
              1{selectedInfo.subagents.length > 0 ? ` + ${selectedInfo.subagents.length}` : ""}
            </span>
          </div>

          <div className="overflow-y-auto" style={{ height: `${agentPct}%` }}>
            {/* "All events" option */}
            <button
              data-testid="agent-all"
              onClick={() => onFocusAgent(null)}
              className={`w-full text-left px-3 py-1.5 text-xs border-b border-[color:var(--bg-hover)] transition-colors ${
                !focusAgentId ? "bg-[color:var(--bg-surface)] text-[color:var(--accent)]" : "text-[color:var(--text-muted)] hover:bg-[color:var(--bg-surface)]"
              }`}
            >
              All events ({selectedInfo.eventCount})
            </button>

            {/* Main agent — always present */}
            <button
              data-testid="agent-main"
              onClick={() => onFocusAgent("__main__")}
              className={`w-full text-left px-3 py-1.5 border-b border-[color:var(--bg-hover)] transition-colors ${
                focusAgentId === "__main__"
                  ? "bg-[color:var(--bg-surface)] border-l-2 border-l-[color:var(--accent)]"
                  : "hover:bg-[color:var(--bg-surface)]"
              }`}
            >
              <div className="flex items-center gap-1.5">
                <span className="text-[length:var(--fs-body)] text-[color:var(--accent)]">Main agent</span>
                <span className="text-[length:var(--fs-label)] text-[color:var(--text-muted)]">
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
                  className={`w-full text-left pl-6 pr-3 py-1.5 border-b border-[color:var(--bg-hover)] transition-colors ${
                    isActive
                      ? "bg-[#1a1a3e] border-l-2 border-l-[color:var(--purple)]"
                      : "hover:bg-[color:var(--bg-surface)]"
                  }`}
                >
                  <div className="flex items-center gap-1.5">
                    <span className="text-[length:var(--fs-label)] text-[color:var(--text-muted)]">└</span>
                    <span className="text-[length:var(--fs-body)] text-[color:var(--purple)] truncate" title={sub.description ?? sub.agentId}>
                      {sub.description ?? sub.agentId.slice(0, 16)}
                    </span>
                  </div>
                  <div className="text-[length:var(--fs-label)] text-[color:var(--text-muted)] pl-4">
                    {sub.eventCount} events · <Timestamp iso={sub.firstTimestamp} />
                  </div>
                </button>
              );
            })}

            {selectedInfo.subagents.length === 0 && (
              <div className="px-3 py-2 text-[length:var(--fs-label)] text-[color:var(--text-muted)] italic">
                No subagents spawned
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
});
