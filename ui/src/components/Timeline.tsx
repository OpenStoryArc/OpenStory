/**
 * Live Timeline — the primary UI.
 *
 * Shows all events from all sessions as a single scrolling stream.
 * Pure function drives it: state.records → toTimelineRows() → rendered rows.
 */

import { useRef, useEffect, useState, useMemo, useCallback, memo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { Observable } from "rxjs";
import { useObservable } from "@/hooks/use-observable";
import type { EnrichedSessionState } from "@/streams/sessions";
import type { WireRecord } from "@/types/wire-record";
import { toTimelineRows, type TimelineRow, type TimelineCategory } from "@/lib/timeline";
import { compactTime } from "@/lib/time";
import { CardBody } from "@/components/events/EventCard";
import { TIMELINE_FILTERS, FILTER_GROUPS } from "@/lib/timeline-filters";
import { FILTER_LABELS, FILTER_TOOLTIPS, PATTERN_LABELS, PATTERN_TOOLTIPS } from "@/lib/ui-labels";
import { shouldClearFocus } from "@/lib/focus";
import { emptyStateMessage } from "@/lib/empty-state";
import { useConnectionStatus } from "@/hooks/use-connection-status";
import { subtreeIds } from "@/lib/subtree";
import { buildPatternIndex } from "@/lib/pattern-index";
import { extractTurnPhases } from "@/lib/turn-phases";
import { TurnPhaseBar } from "@/components/TurnPhaseBar";
import type { PatternView } from "@/types/wire-record";
import { computeTurnSummaries, type TurnSummary } from "@/lib/turn-summary";
import { GitFlowCard } from "@/components/events/GitFlowCard";

// ---------------------------------------------------------------------------
// Color palette for category badges (Tokyonight)
// ---------------------------------------------------------------------------
const CATEGORY_COLORS: Record<TimelineCategory, string> = {
  prompt: "#7aa2f7",
  response: "#bb9af7",
  thinking: "#9ece6a",
  tool: "#2ac3de",
  result: "#2ac3de",
  system: "#565f89",
  error: "#f7768e",
  turn: "#3b4261",
};

const CATEGORY_LABELS: Record<TimelineCategory, string> = {
  prompt: "Prompt",
  response: "Response",
  thinking: "Thinking",
  tool: "Tool",
  result: "Result",
  system: "System",
  error: "Error",
  turn: "Turn",
};

// ---------------------------------------------------------------------------
// Session badge — short colored identifier
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
// Pattern badge colors (Tokyonight palette)
// ---------------------------------------------------------------------------
const PATTERN_COLORS: Record<string, string> = {
  "test.cycle": "#9ece6a",      // green (pass) — red handled via metadata
  "git.workflow": "#7aa2f7",    // blue
  "error.recovery": "#e0af68",  // orange
  "agent.delegation": "#bb9af7", // purple
  "turn.phase": "#565f89",      // muted grey
};

function patternColor(p: PatternView): string {
  return PATTERN_COLORS[p.type] ?? "#565f89";
}

function formatTurnDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  const mins = Math.floor(ms / 60000);
  const secs = Math.round((ms % 60000) / 1000);
  return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
}

/** Height estimate for virtualizer. ResizeObserver corrects after render. */
function estimateCardHeight(row: TimelineRow): number {
  if (row.category === "turn") return 28;
  // Estimate from text length: ~80 chars/line, ~18px/line, +48px padding
  const lines = Math.max(1, Math.ceil(row.summary.length / 80));
  return 48 + lines * 18;
}

// ---------------------------------------------------------------------------
// Session avatar — colored circle with 3-char ID
// ---------------------------------------------------------------------------
function SessionAvatar({ sessionId, label }: { sessionId: string; label?: string | null }) {
  const color = sessionColor(sessionId);
  return (
    <div
      className="w-8 h-8 rounded-full shrink-0 flex items-center justify-center text-[10px] font-bold"
      style={{ backgroundColor: `${color}25`, color }}
      title={label || sessionId}
    >
      {sessionId.slice(0, 3).toUpperCase()}
    </div>
  );
}

// CardBody imported from @/components/events/EventCard

// ---------------------------------------------------------------------------
// TimelineRowView — single unified card. The card IS the content.
// ---------------------------------------------------------------------------
interface RowProps {
  row: TimelineRow;
  isFocusRoot: boolean;
  isHighlighted: boolean;
  patterns: readonly PatternView[];
  turnSummary: TurnSummary | null;
  sessionLabel: string | null;
  onPatternClick: (pattern: PatternView) => void;
  onExploreLink?: (sessionId: string) => void;
}

const TimelineRowView = memo(function TimelineRowView({ row, isFocusRoot, isHighlighted, patterns, turnSummary, sessionLabel, onPatternClick, onExploreLink }: RowProps) {
  const catColor = CATEGORY_COLORS[row.category];

  // Turn divider
  if (row.category === "turn") {
    const parts: string[] = [];
    if (turnSummary) {
      if (turnSummary.durationMs != null) parts.push(formatTurnDuration(turnSummary.durationMs));
      if (turnSummary.toolCalls > 0) parts.push(`${turnSummary.toolCalls} tool${turnSummary.toolCalls !== 1 ? "s" : ""}`);
      if (turnSummary.edits > 0) parts.push(`${turnSummary.edits} edit${turnSummary.edits !== 1 ? "s" : ""}`);
      if (turnSummary.errors > 0) parts.push(`${turnSummary.errors} error${turnSummary.errors !== 1 ? "s" : ""}`);
    }
    return (
      <div className="flex items-center px-4 py-2" data-testid="turn-divider">
        <div className="flex-1 h-px bg-[#3b4261]" />
        <span className="text-[10px] text-[#565f89] px-3 shrink-0 font-mono">
          {parts.length > 0 ? parts.join(" · ") : row.summary}
        </span>
        <div className="flex-1 h-px bg-[#3b4261]" />
      </div>
    );
  }

  const highlight = isHighlighted ? " bg-[#7aa2f714]" : "";
  const focusBorder = isFocusRoot ? " ring-1 ring-[#e0af68]" : "";

  return (
    <div
      className={`mx-3 my-1 rounded-xl border border-[#2f3348] overflow-hidden hover:border-[#414868]${highlight}${focusBorder}`}
      data-testid="timeline-row"
    >
      <div className="px-3 py-2">
        <div className="flex gap-3">
          <SessionAvatar sessionId={row.sessionId} label={sessionLabel} />
          <div className="flex-1 min-w-0">
            {/* Header */}
            <div className="flex items-center gap-1.5 mb-1 flex-wrap">
              {sessionLabel && (
                <span className="text-[11px] text-[#c0caf5] font-medium truncate max-w-[200px]">
                  {sessionLabel}
                </span>
              )}
              <span
                className="text-[10px] px-1.5 py-0.5 rounded font-medium"
                style={{ color: catColor, backgroundColor: `${catColor}18` }}
                data-testid="row-category-badge"
              >
                {CATEGORY_LABELS[row.category]}
              </span>
              {row.toolName && (
                <span className="text-xs font-semibold text-[#2ac3de]">{row.toolName}</span>
              )}
              {patterns.map((p, i) => {
                const color = patternColor(p);
                return (
                  <span
                    key={`${p.type}-${i}`}
                    role="button"
                    onClick={(e) => { e.stopPropagation(); onPatternClick(p); }}
                    className="text-[9px] px-1.5 py-0.5 rounded-full border cursor-pointer hover:brightness-125"
                    style={{ color, backgroundColor: `${color}10`, borderColor: `${color}40` }}
                    title={PATTERN_TOOLTIPS[p.type] ?? p.label}
                    data-testid="pattern-badge"
                  >
                    {PATTERN_LABELS[p.type] ?? p.type}
                  </span>
                );
              })}
              <span className="ml-auto flex items-center gap-1.5 shrink-0">
                <span className="text-[10px] text-[#565f89] font-mono">
                  {compactTime(row.timestamp)}
                </span>
                {onExploreLink && (
                  <button
                    onClick={(e) => { e.stopPropagation(); onExploreLink(row.sessionId); }}
                    className="text-[11px] px-1.5 py-0.5 rounded text-[#565f89] hover:text-[#7aa2f7] hover:bg-[#7aa2f710] transition-colors"
                    title="Open full session in Explore"
                    data-testid="explore-link"
                  >
                    Explore ↗
                  </button>
                )}
              </span>
            </div>

            {/* Body — the card IS the content */}
            <CardBody row={row} />
          </div>
        </div>
      </div>
    </div>
  );
});

// ---------------------------------------------------------------------------
// FilterBar — compact grouped filter picker
// ---------------------------------------------------------------------------
interface FilterBarProps {
  activeFilter: string;
  onSelect: (filter: string) => void;
  matchCount: number;
  totalCount: number;
  filterCounts: Readonly<Record<string, number>>;
}

const FilterBar = memo(function FilterBar({ activeFilter, onSelect, matchCount, totalCount, filterCounts }: FilterBarProps) {
  const filters = FILTER_GROUPS.flatMap((g) => g.filters);
  return (
    <div className="px-3 py-1.5 bg-[#1a1b26] border-b border-[#2f3348] text-xs" data-testid="filter-bar">
      <div className="flex items-center gap-1 flex-wrap">
        {filters.map((f) => {
              const count = filterCounts[f];
              return (
                <button
                  key={f}
                  onClick={() => onSelect(f)}
                  data-testid={`filter-${f}`}
                  title={FILTER_TOOLTIPS[f]}
                  className={`px-2 py-1 rounded text-[11px] transition-colors ${
                    activeFilter === f
                      ? "bg-[#7aa2f7] text-[#1a1b26] font-medium"
                      : "text-[#787c99] hover:text-[#c0caf5] hover:bg-[#24283b]"
                  }`}
                >
                  {FILTER_LABELS[f] ?? f}
                  {count != null && count > 0 && f !== "all" && (
                    <span className="ml-0.5 text-[9px] opacity-60">{count}</span>
                  )}
                </button>
              );
        })}
        {activeFilter !== "all" && (
          <span className="text-[#565f89] ml-auto" data-testid="filter-match-count">
            {matchCount}/{totalCount}
          </span>
        )}
      </div>
    </div>
  );
});

// ---------------------------------------------------------------------------
// Timeline — the main component
// ---------------------------------------------------------------------------
interface TimelineProps {
  state$: Observable<EnrichedSessionState>;
  /** Filter to a single session (null = all sessions) */
  sessionFilter?: string | null;
  /** Filter to a single agent (null = all agents) */
  agentFilter?: string | null;
  /** Callback when user clicks "explore" on a card */
  onExploreLink?: (link: import("@/lib/navigation").CrossLink) => void;
}

export function Timeline({ state$, sessionFilter = null, agentFilter = null, onExploreLink }: TimelineProps) {
  const state = useObservable(state$, { records: [], currentEphemeral: null, patterns: [], filterCounts: {}, treeIndex: new Map(), sessionLabels: {}, agentLabels: {} } as EnrichedSessionState);
  const connectionStatus = useConnectionStatus();
  const [activeFilter, setActiveFilter] = useState("all");
  const [focusRootId, setFocusRootId] = useState<string | null>(null);

  // Build subtree membership set using treeIndex from state (null when not focused)
  const subtreeSet = useMemo(() => {
    if (!focusRootId) return null;
    // Convert treeIndex to id → parent_uuid map for subtreeIds
    const parentIndex = new Map<string, string | null>();
    for (const [id, { parent_uuid }] of state.treeIndex) {
      parentIndex.set(id, parent_uuid);
    }
    return subtreeIds(focusRootId, parentIndex);
  }, [state.treeIndex, focusRootId]);

  // Apply session/agent/subtree/filter, then transform to timeline rows
  const { rows, matchCount } = useMemo(() => {
    const predicate = TIMELINE_FILTERS[activeFilter] ?? TIMELINE_FILTERS["all"]!;
    let filtered: readonly WireRecord[] = state.records;

    // Session filter from sidebar
    if (sessionFilter) {
      filtered = filtered.filter((ev) => ev.session_id === sessionFilter);
    }

    // Agent filter from sidebar
    if (agentFilter === "__main__") {
      filtered = filtered.filter((ev) => ev.agent_id === null);
    } else if (agentFilter) {
      filtered = filtered.filter((ev) => ev.agent_id === agentFilter);
    }

    if (subtreeSet) {
      filtered = filtered.filter((ev) => subtreeSet.has(ev.id));
    }
    if (activeFilter !== "all") {
      filtered = filtered.filter((ev) => predicate(ev));
    }
    return { rows: toTimelineRows(filtered), matchCount: filtered.length };
  }, [state.records, activeFilter, subtreeSet, sessionFilter, agentFilter]);

  // Auto-clear focus when focused event is no longer in visible rows (Story 046).
  // This handles: filter changes, session switches, agent filter changes.
  // Uses rows (post-filter) so focus clears when the event is filtered out.
  useEffect(() => {
    if (focusRootId) {
      const visibleIds = new Set(rows.map((r) => r.id));
      if (shouldClearFocus(focusRootId, visibleIds)) {
        setFocusRootId(null);
      }
    }
  }, [rows, focusRootId]);

  const handleFilterSelect = useCallback((f: string) => setActiveFilter(f), []);

  // Compute filter counts client-side from visible records
  const derivedFilterCounts = useMemo(() => {
    let visible: readonly WireRecord[] = state.records;
    if (sessionFilter) visible = visible.filter((r) => r.session_id === sessionFilter);
    if (agentFilter === "__main__") visible = visible.filter((r) => r.agent_id === null);
    else if (agentFilter) visible = visible.filter((r) => r.agent_id === agentFilter);

    const counts: Record<string, number> = {};
    for (const [name, predicate] of Object.entries(TIMELINE_FILTERS)) {
      if (name === "all") continue;
      let count = 0;
      for (const r of visible) { if (predicate(r)) count++; }
      counts[name] = count;
    }
    return counts;
  }, [state.records, sessionFilter, agentFilter]);


  // Compute turn summaries for turn divider rows
  const turnSummaries = useMemo(
    () => computeTurnSummaries(state.records),
    [state.records],
  );

  // Build pattern index: event_id → patterns
  const patternIndex = useMemo(
    () => buildPatternIndex(state.patterns),
    [state.patterns],
  );

  // Extract turn phase segments for the phase bar
  const turnPhases = useMemo(
    () => extractTurnPhases(state.patterns),
    [state.patterns],
  );

  // Pattern click-to-highlight: clicking a pattern badge highlights all member events
  const [highlightedPattern, setHighlightedPattern] = useState<PatternView | null>(null);
  const highlightedEventIds = useMemo(() => {
    if (!highlightedPattern) return new Set<string>();
    return new Set(highlightedPattern.events);
  }, [highlightedPattern]);

  const handlePatternClick = useCallback((p: PatternView) => {
    setHighlightedPattern((prev) =>
      prev && prev.type === p.type && prev.events[0] === p.events[0] ? null : p,
    );
  }, []);

  const handleExploreLink = useCallback((sessionId: string) => {
    onExploreLink?.({ sessionId });
  }, [onExploreLink]);

  // Count unique sessions from records
  const sessionCount = useMemo(() => {
    const ids = new Set<string>();
    for (const r of state.records) {
      if (r.session_id) ids.add(r.session_id);
    }
    return ids.size;
  }, [state.records]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const prevCountRef = useRef(0);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: useCallback(
      (index: number) => {
        const row = rows[index];
        if (!row) return 48;
        return estimateCardHeight(row);
      },
      [rows],
    ),
    overscan: 15,
    getItemKey: useCallback((index: number) => rows[index]?.id ?? index, [rows]),
  });

  // Auto-scroll to top when new rows arrive (newest-first order).
  // rAF-gated: at most 1 scroll per frame, eliminating CLS from scroll commands.
  const scrollRafRef = useRef(0);
  useEffect(() => {
    if (autoScroll && rows.length > prevCountRef.current) {
      cancelAnimationFrame(scrollRafRef.current);
      scrollRafRef.current = requestAnimationFrame(() => {
        virtualizer.scrollToIndex(0, { align: "start" });
      });
    }
    prevCountRef.current = rows.length;
    return () => cancelAnimationFrame(scrollRafRef.current);
  }, [rows.length, autoScroll, virtualizer]);

  // No expand/collapse state — cards are always full content.
  // Virtualizer relies on measureElement + ResizeObserver for accurate heights.

  // Detect manual scroll to disable auto-scroll
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const onScroll = () => {
      const atTop = el.scrollTop < 50;
      setAutoScroll(atTop);
    };

    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <div className="flex flex-col h-full" data-testid="timeline">
      {/* Status bar */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-[#24283b] border-b border-[#2f3348] text-xs text-[#565f89]" data-testid="timeline-status">
        <span className="flex items-center gap-2">
          {highlightedPattern && (
            <span className="flex items-center gap-1" data-testid="highlight-indicator">
              <span style={{ color: patternColor(highlightedPattern) }}>
                {PATTERN_LABELS[highlightedPattern.type] ?? highlightedPattern.type}: {highlightedPattern.events.length} events
              </span>
              {highlightedPattern.type === "git.workflow" && highlightedPattern.metadata && (
                <GitFlowCard metadata={highlightedPattern.metadata} />
              )}
              <button
                onClick={() => setHighlightedPattern(null)}
                className="text-[#565f89] hover:text-[#f7768e] ml-0.5"
                title="Clear highlight"
              >
                ×
              </button>
              <span className="text-[#2f3348]">·</span>
            </span>
          )}
          {focusRootId && (
            <span className="flex items-center gap-1">
              <span className="text-[#e0af68]">Focused on {focusRootId.slice(0, 8)}</span>
              <button
                onClick={() => setFocusRootId(null)}
                className="text-[#565f89] hover:text-[#f7768e] ml-0.5"
                title="Exit focus"
              >
                ×
              </button>
              <span className="text-[#2f3348]">·</span>
            </span>
          )}
          <span>
            {matchCount === state.records.length
              ? `${rows.length} events`
              : `${rows.length} of ${state.records.length} events`}
            {sessionCount > 0 && ` from ${sessionCount} session${sessionCount !== 1 ? "s" : ""}`}
            {state.patterns.length > 0 && (
              <span
                className="text-[#bb9af7]"
                data-testid="pattern-count"
                title="Behavioral patterns detected: test cycles, git workflows, error recoveries, agent delegations, and turn phases"
              >
                {` · ${state.patterns.length} pattern${state.patterns.length !== 1 ? "s" : ""}`}
              </span>
            )}
          </span>
        </span>
        {!autoScroll && (
          <button
            onClick={() => {
              setAutoScroll(true);
              virtualizer.scrollToIndex(0, { align: "start" });
            }}
            className="text-[#7aa2f7] hover:text-[#89b4fa]"
          >
            Scroll to latest
          </button>
        )}
      </div>

      {/* Disconnected banner — warns user data may be stale */}
      {connectionStatus === "disconnected" && state.records.length > 0 && (
        <div className="px-3 py-1.5 bg-[#f7768e15] border-b border-[#f7768e30] text-xs text-[#f7768e]" data-testid="disconnected-banner">
          Connection lost — data may be stale. Waiting to reconnect...
        </div>
      )}

      {/* Ephemeral progress indicator */}
      {state.currentEphemeral && (
        <div className="px-3 py-1 bg-[#1e2030] border-b border-[#2f3348] text-xs text-[#e0af68] animate-pulse">
          {state.currentEphemeral.record_type === "system_event"
            ? ((state.currentEphemeral.payload as { message?: string }).message ?? "Working...")
            : "Working..."}
        </div>
      )}

      {/* Turn phase bar */}
      <TurnPhaseBar segments={turnPhases} />

      {/* Filter bar */}
      <FilterBar
        activeFilter={activeFilter}
        onSelect={handleFilterSelect}
        matchCount={matchCount}
        totalCount={state.records.length}
        filterCounts={derivedFilterCounts}
      />

      {/* Event feed */}
      <div ref={scrollRef} className="flex-1 overflow-auto">
        {rows.length === 0 ? (
          (() => {
            const msg = emptyStateMessage({
              connection: connectionStatus,
              activeFilter,
              totalRecords: state.records.length,
            });
            return (
              <div className="flex items-center justify-center h-full text-[#565f89]" data-testid="empty-state">
                <div className="text-center">
                  <div className="text-lg mb-2">{msg.headline}</div>
                  <div className="text-xs">{msg.detail}</div>
                  {msg.action && (
                    <button
                      onClick={() => setActiveFilter(msg.action!)}
                      className="mt-3 text-xs text-[#7aa2f7] hover:text-[#89b4fa] underline"
                      data-testid="empty-state-action"
                    >
                      Show all events
                    </button>
                  )}
                </div>
              </div>
            );
          })()
        ) : (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative", overflow: "hidden" }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const row = rows[virtualRow.index]!;
              return (
                <div
                  key={row.id}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                  ref={virtualizer.measureElement}
                  data-index={virtualRow.index}
                >
                  <TimelineRowView
                    row={row}
                    isFocusRoot={focusRootId === row.id}
                    isHighlighted={highlightedEventIds.has(row.id)}
                    patterns={patternIndex.get(row.id) ?? []}
                    turnSummary={row.category === "turn" ? (turnSummaries.get(row.id) ?? null) : null}
                    sessionLabel={state.sessionLabels[row.sessionId]?.label ?? null}
                    onPatternClick={handlePatternClick}
                    onExploreLink={onExploreLink ? handleExploreLink : undefined}
                  />
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
