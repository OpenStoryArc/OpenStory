/**
 * Live Timeline — the primary UI.
 *
 * Shows all events from all sessions as a single scrolling stream.
 * Pure function drives it: state.records → toTimelineRows() → rendered rows.
 */

import { sessionColor, tint } from "@/lib/session-colors";
import { useRef, useEffect, useState, useMemo, useCallback, memo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { Observable } from "rxjs";
import { useObservable } from "@/hooks/use-observable";
import type { EnrichedSessionState } from "@/streams/sessions";
import { dispatchSessionRecordsLoaded } from "@/streams/sessions";
import { streamSessionRecords } from "@/lib/session-records";
import type { WireRecord } from "@/types/wire-record";
import { toTimelineRows, type TimelineRow, type TimelineCategory } from "@/lib/timeline";
import { compactTime, fullTimestamp } from "@/lib/time";
import { CardBody } from "@/components/events/EventCard";
import { TIMELINE_FILTERS, FILTER_GROUPS } from "@/lib/timeline-filters";
import { FILTER_LABELS, FILTER_TOOLTIPS, PATTERN_LABELS, PATTERN_TOOLTIPS } from "@/lib/ui-labels";
import { shouldClearFocus } from "@/lib/focus";
import { emptyStateMessage } from "@/lib/empty-state";
import { useConnectionStatus } from "@/hooks/use-connection-status";
import { subtreeIds } from "@/lib/subtree";
import { nextCardIndex } from "@/lib/keyboard-nav";
import { buildPatternIndex } from "@/lib/pattern-index";
import { summarizePatterns, patternRollupLabel } from "@/lib/pattern-rollup";
import { extractTurnPhases } from "@/lib/turn-phases";
import { TurnPhaseBar } from "@/components/TurnPhaseBar";
import type { PatternView } from "@/types/wire-record";
import { computeTurnSummaries, type TurnSummary } from "@/lib/turn-summary";

// ---------------------------------------------------------------------------
// Color palette for category badges (Tokyonight)
// ---------------------------------------------------------------------------
// Theme-aware: semantic vars, not literal pastels — kind colors stay legible
// in light mode and echo the activity ribbon's lane language.
const CATEGORY_COLORS: Record<TimelineCategory, string> = {
  prompt: "var(--accent)",
  response: "var(--purple)",
  thinking: "var(--green)",
  tool: "var(--cyan)",
  result: "var(--cyan)",
  system: "var(--text-muted)",
  error: "var(--red)",
  turn: "var(--border)",
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

// Pattern badge colors (Tokyonight palette)
// ---------------------------------------------------------------------------
// All persisted pattern types live in the eval_apply.* / turn.* namespaces.
// Pre-cleanup, this map had entries for legacy types (test.cycle, git.workflow,
// error.recovery, agent.delegation, turn.phase) — all retired in
// chore/cut-legacy-detectors. Anything not matched falls back to the muted
// grey default below, which keeps the badge palette consistent without
// requiring per-type entries.
const PATTERN_COLORS: Record<string, string> = {};

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
      style={{ backgroundColor: tint(color, 15), color }}
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
  isSelected: boolean;
  patterns: readonly PatternView[];
  turnSummary: TurnSummary | null;
  sessionLabel: string | null;
  onPatternClick: (pattern: PatternView) => void;
  onSelect?: () => void;
  onExploreLink?: (sessionId: string, eventId: string) => void;
}

const TimelineRowView = memo(function TimelineRowView({ row, isFocusRoot, isHighlighted, isSelected, patterns, turnSummary, sessionLabel, onPatternClick, onSelect, onExploreLink }: RowProps) {
  const catColor = CATEGORY_COLORS[row.category];
  // Pattern pills default to a calm rollup ("12 cycles · 8 sentences"); the raw
  // detector pills expand on click (map principle: quiet default, detail on demand).
  const [showPills, setShowPills] = useState(false);
  const rollup = summarizePatterns(patterns);

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
        {/* Sits directly ON the stream field — fixed on-stream inks, not theme text vars. */}
        <div className="flex-1 h-px bg-[color:var(--stream-line)]" />
        <span className="text-[10px] text-[color:var(--stream-text)] px-3 shrink-0 font-mono">
          {parts.length > 0 ? parts.join(" · ") : row.summary}
        </span>
        <div className="flex-1 h-px bg-[color:var(--stream-line)]" />
      </div>
    );
  }

  const highlight = isHighlighted ? " bg-[color:var(--accent)]/8" : "";
  const focusBorder = isFocusRoot ? " ring-1 ring-[color:var(--orange)]" : "";
  const selectedBorder = isSelected ? " ring-1 ring-[color:var(--accent)]" : "";

  return (
    <div
      className={`mx-3 my-1 rounded-lg border border-[color:var(--divider)] overflow-hidden hover:border-[color:var(--border)]${highlight}${focusBorder}${selectedBorder} cursor-pointer`}
      data-testid="timeline-row"
      onClick={onSelect}
      // Markdown-block idiom: each event is a distinct block — a colored kind
      // gutter (blockquote/diff style) + a whisper of kind color washed over
      // an ELEVATED surface, so cards visibly lift off the stream background.
      // Highlight state keeps its own background.
      style={{
        borderLeft: `3px solid ${catColor}`,
        ...(isHighlighted
          ? {}
          : { background: `color-mix(in oklab, ${catColor} 5%, var(--bg-surface))` }),
      }}
    >
      <div className="px-3 py-2">
        <div className="flex gap-3">
          <SessionAvatar sessionId={row.sessionId} label={sessionLabel} />
          <div className="flex-1 min-w-0">
            {/* Header */}
            <div className="flex items-center gap-1.5 mb-1 flex-wrap">
              {sessionLabel && (
                <span className="text-[11px] text-[color:var(--text)] font-medium truncate max-w-[200px]">
                  {sessionLabel}
                </span>
              )}
              <span
                className="text-[10px] px-1.5 py-0.5 rounded font-medium"
                style={{ color: catColor, backgroundColor: tint(catColor, 10) }}
                data-testid="row-category-badge"
              >
                {CATEGORY_LABELS[row.category]}
              </span>
              {row.toolName && (
                <span className="text-xs font-semibold text-[color:var(--cyan)]">{row.toolName}</span>
              )}
              {rollup.total > 0 && (
                showPills ? (
                  <>
                    {patterns.map((p, i) => {
                      const color = patternColor(p);
                      return (
                        <span
                          key={`${p.type}-${i}`}
                          role="button"
                          onClick={(e) => { e.stopPropagation(); onPatternClick(p); }}
                          className="text-[9px] px-1.5 py-0.5 rounded-full border cursor-pointer hover:brightness-125"
                          style={{ color, backgroundColor: tint(color, 6), borderColor: tint(color, 25) }}
                          title={PATTERN_TOOLTIPS[p.type] ?? p.label}
                          data-testid="pattern-badge"
                        >
                          {PATTERN_LABELS[p.type] ?? p.type}
                        </span>
                      );
                    })}
                    <button
                      onClick={(e) => { e.stopPropagation(); setShowPills(false); }}
                      className="text-[9px] text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
                    >
                      ◂ collapse
                    </button>
                  </>
                ) : (
                  <button
                    onClick={(e) => { e.stopPropagation(); setShowPills(true); }}
                    className="text-[9px] px-1.5 py-0.5 rounded-full border border-[color:var(--border)] text-[color:var(--accent)] hover:bg-[color:var(--accent)]/6"
                    title="Show the raw pattern detections"
                    data-testid="pattern-rollup"
                  >
                    {patternRollupLabel(rollup) || `${rollup.total} patterns`} ▸
                  </button>
                )
              )}
              <span className="ml-auto flex items-center gap-1.5 shrink-0">
                <span className="text-[10px] text-[color:var(--text-muted)] font-mono">
                  <span title={fullTimestamp(row.timestamp)}>{compactTime(row.timestamp)}</span>
                </span>
                {onExploreLink && (
                  <button
                    onClick={(e) => { e.stopPropagation(); onExploreLink(row.sessionId, row.id); }}
                    className="text-[11px] px-1.5 py-0.5 rounded text-[color:var(--text-muted)] hover:text-[color:var(--accent)] hover:bg-[color:var(--accent)]/6 transition-colors"
                    title="Open full session in Explore"
                    data-testid="explore-link"
                  >
                    Explore ↗
                  </button>
                )}
              </span>
            </div>

            {/* Body — the CONTENT sits in its own inset well (Max: a box
                around the event's content), visibly recessed from the card's
                header chrome: page-toned wash + soft inner border. Dark mode
                recesses toward ink, sepia toward parchment — both themes get
                the content/chrome separation for free from the tokens. */}
            <div className="mt-1.5 rounded-md border border-[color:var(--divider)]/50 bg-[color:var(--bg)]/45 px-2.5 py-2">
              <CardBody row={row} />
              {/* Full IDs — cross-referencing metadata lives with the content */}
              <div className="mt-1.5 border-t border-[color:var(--divider)]/40 pt-1 text-[9px] text-[color:var(--text-muted)] font-mono leading-tight">
                <div>event: {row.id}</div>
                <div>session: {row.sessionId}</div>
              </div>
            </div>
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
    <div className="px-3 py-1.5 bg-[color:var(--bg)] border-b border-[color:var(--divider)] text-xs" data-testid="filter-bar">
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
                      ? "bg-[color:var(--accent)] text-[color:var(--bg)] font-medium"
                      : "text-[#787c99] hover:text-[color:var(--text)] hover:bg-[color:var(--bg-surface)]"
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
          <span className="text-[color:var(--text-muted)] ml-auto" data-testid="filter-match-count">
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
  const state = useObservable(state$, { records: [], currentEphemeral: null, patterns: [], treeIndex: new Map(), sessionLabels: {}, loadedSessions: new Set() } as EnrichedSessionState);
  const connectionStatus = useConnectionStatus();
  const [activeFilter, setActiveFilter] = useState("all");
  const [focusRootId, setFocusRootId] = useState<string | null>(null);
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [timelineFocused, setTimelineFocused] = useState(false);
  const [loadingSession, setLoadingSession] = useState(false);

  // ── Lazy-load: stream records for the selected session via REST ────
  //
  // Pre-redesign, the WebSocket handshake shipped every session's records
  // (~39 MB on a real box). After feat/lazy-load-initial-state, records
  // arrive lazily on session-open. We `streamSessionRecords()` and
  // dispatch each page as it arrives so the user sees the recent
  // activity after one round-trip; older history fills in behind it.
  // The reducer dedups by id, so live `enriched` deltas overlapping the
  // in-flight stream are preserved.
  //
  // The cache check uses a ref instead of the state value as a dep:
  // dispatching the first page mutates `state.loadedSessions`, and
  // re-running this effect mid-stream would abort the iterator.
  const loadedSessionsRef = useRef(state.loadedSessions);
  loadedSessionsRef.current = state.loadedSessions;

  useEffect(() => {
    if (!sessionFilter) return;
    if (loadedSessionsRef.current.has(sessionFilter)) return;

    const ctrl = new AbortController();
    setLoadingSession(true);

    (async () => {
      try {
        for await (const page of streamSessionRecords(sessionFilter, {
          signal: ctrl.signal,
        })) {
          if (ctrl.signal.aborted) return;
          dispatchSessionRecordsLoaded(sessionFilter, page);
        }
      } catch (err) {
        if (ctrl.signal.aborted) return;
        // eslint-disable-next-line no-console
        console.warn("[timeline] session records stream failed:", err);
      } finally {
        if (!ctrl.signal.aborted) setLoadingSession(false);
      }
    })();

    return () => ctrl.abort();
  }, [sessionFilter]);

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

  // Derive turn phase segments from records on the fly. The legacy
  // `turn.phase` pattern type was retired — phases are a runtime projection
  // of the conversation's tool usage, not persisted derived data.
  const turnPhases = useMemo(
    () => extractTurnPhases(state.records),
    [state.records],
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

  const handleExploreLink = useCallback((sessionId: string, eventId: string) => {
    onExploreLink?.({ sessionId, eventId });
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

  // Clear selection when rows change (filter, session switch, new data)
  const rowsRef = useRef(rows);
  useEffect(() => {
    if (rowsRef.current !== rows) {
      setSelectedIndex(null);
      rowsRef.current = rows;
    }
  }, [rows]);

  // Keyboard navigation: up/down arrows move between event cards.
  // Uses a ref to avoid side effects inside the state updater,
  // and rAF to batch the scroll with the next paint frame.
  const selectedIndexRef = useRef(selectedIndex);
  selectedIndexRef.current = selectedIndex;
  const navRafRef = useRef(0);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        const sidebar = document.querySelector<HTMLElement>('[data-focus-zone="sidebar"]');
        sidebar?.focus();
        return;
      }
      if (e.key === "Enter" && selectedIndexRef.current !== null && onExploreLink) {
        e.preventDefault();
        const row = rows[selectedIndexRef.current];
        if (row && row.category !== "turn") {
          onExploreLink({ sessionId: row.sessionId, eventId: row.id });
        }
        return;
      }
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
      e.preventDefault();
      const direction = e.key === "ArrowDown" ? "down" : "up";
      const next = nextCardIndex(rows, selectedIndexRef.current, direction);
      if (next === null || next === selectedIndexRef.current) return;
      setSelectedIndex(next);
      setAutoScroll(false);
      cancelAnimationFrame(navRafRef.current);
      navRafRef.current = requestAnimationFrame(() => {
        virtualizer.scrollToIndex(next, { align: "center" });
      });
    };

    el.addEventListener("keydown", onKeyDown);
    return () => {
      el.removeEventListener("keydown", onKeyDown);
      cancelAnimationFrame(navRafRef.current);
    };
  }, [rows, virtualizer, onExploreLink]);

  return (
    <div className="flex flex-col h-full" data-testid="timeline">
      {/* Status bar */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-[color:var(--bg-surface)] border-b border-[color:var(--divider)] text-xs text-[color:var(--text-muted)]" data-testid="timeline-status">
        <span className="flex items-center gap-2">
          {highlightedPattern && (
            <span className="flex items-center gap-1" data-testid="highlight-indicator">
              <span style={{ color: patternColor(highlightedPattern) }}>
                {PATTERN_LABELS[highlightedPattern.type] ?? highlightedPattern.type}: {highlightedPattern.events.length} events
              </span>
              <button
                onClick={() => setHighlightedPattern(null)}
                className="text-[color:var(--text-muted)] hover:text-[color:var(--red)] ml-0.5"
                title="Clear highlight"
              >
                ×
              </button>
              <span className="text-[color:var(--bg-hover)]">·</span>
            </span>
          )}
          {focusRootId && (
            <span className="flex items-center gap-1">
              <span className="text-[color:var(--orange)]">Focused on {focusRootId.slice(0, 8)}</span>
              <button
                onClick={() => setFocusRootId(null)}
                className="text-[color:var(--text-muted)] hover:text-[color:var(--red)] ml-0.5"
                title="Exit focus"
              >
                ×
              </button>
              <span className="text-[color:var(--bg-hover)]">·</span>
            </span>
          )}
          <span>
            {matchCount === state.records.length
              ? `${rows.length} events`
              : `${rows.length} of ${state.records.length} events`}
            {sessionCount > 0 && ` from ${sessionCount} session${sessionCount !== 1 ? "s" : ""}`}
            {state.patterns.length > 0 && (
              <span
                className="text-[color:var(--purple)]"
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
            className="text-[color:var(--accent)] hover:text-[color:var(--accent)]"
          >
            Scroll to latest
          </button>
        )}
      </div>

      {/* Disconnected banner — warns user data may be stale */}
      {connectionStatus === "disconnected" && state.records.length > 0 && (
        <div className="px-3 py-1.5 bg-[color:var(--red)]/8 border-b border-[color:var(--red)]/19 text-xs text-[color:var(--red)]" data-testid="disconnected-banner">
          Connection lost — data may be stale. Waiting to reconnect...
        </div>
      )}

      {/* Ephemeral progress indicator */}
      {state.currentEphemeral && (
        <div className="px-3 py-1 bg-[color:var(--bg-surface)] border-b border-[color:var(--divider)] text-xs text-[color:var(--orange)] animate-pulse">
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
      <div ref={scrollRef} className="flex-1 overflow-auto outline-none bg-[color:var(--stream)]" tabIndex={0} data-focus-zone="timeline" onFocus={() => setTimelineFocused(true)} onBlur={() => setTimelineFocused(false)}>
        {loadingSession && rows.length === 0 ? (
          <div className="flex items-center justify-center h-full text-[color:var(--stream-text)]" data-testid="loading-session">
            <div className="text-center">
              <div className="text-lg mb-2">Loading session…</div>
              <div className="text-xs">Fetching records via REST</div>
            </div>
          </div>
        ) : rows.length === 0 ? (
          (() => {
            const msg = emptyStateMessage({
              connection: connectionStatus,
              activeFilter,
              totalRecords: state.records.length,
            });
            return (
              <div className="flex items-center justify-center h-full text-[color:var(--text-muted)]" data-testid="empty-state">
                <div className="text-center">
                  <div className="text-lg mb-2">{msg.headline}</div>
                  <div className="text-xs">{msg.detail}</div>
                  {msg.action && (
                    <button
                      onClick={() => setActiveFilter(msg.action!)}
                      className="mt-3 text-xs text-[color:var(--accent)] hover:text-[color:var(--accent)] underline"
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
                    isSelected={timelineFocused && selectedIndex === virtualRow.index}
                    patterns={patternIndex.get(row.id) ?? []}
                    turnSummary={row.category === "turn" ? (turnSummaries.get(row.id) ?? null) : null}
                    sessionLabel={state.sessionLabels[row.sessionId]?.label ?? null}
                    onPatternClick={handlePatternClick}
                    onSelect={() => { setSelectedIndex(virtualRow.index); scrollRef.current?.focus(); }}
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
