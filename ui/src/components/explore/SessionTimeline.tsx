/** Session event timeline with faceted graph navigation.
 *  Left: turn outline + file/tool facets. Right: event cards (compact, click to expand). */

import { useState, useEffect, useRef, useMemo, useCallback } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useSessionRecords } from "@/hooks/use-session-records";
import { toolPairMap } from "@/lib/tool-pair";
import { toTimelineRows } from "@/lib/timeline";
import { filterNoise } from "@/lib/explore-filters";
import { buildEventGraph, applyFacets, fileFacets, toolFacets, planFacets, type ActiveFacets } from "@/lib/event-graph";
import { TurnOutline } from "./TurnOutline";
import { FacetPanel } from "./FacetPanel";
import { EventCardRow } from "@/components/events/EventCard";
import { SessionActivityRibbon } from "@/components/viz/SessionActivityRibbon";
import { TurnTraceView } from "@/components/viz/TurnTraceView";
import { TIMELINE_FILTERS, FILTER_GROUPS } from "@/lib/timeline-filters";
import { FILTER_LABELS } from "@/lib/ui-labels";
import { usePersistedFlag } from "@/hooks/use-persisted-flag";
import { SessionSummaryHeader } from "@/components/viz/SessionSummaryHeader";
import { SessionVizSkeleton } from "@/components/ui/skeletons";
import { firstErrorEventId } from "@/lib/session-summary";
import { nextCardIndex } from "@/lib/keyboard-nav";

interface SessionTimelineProps {
  sessionId: string;
  /** Event ID to scroll into view after records load. */
  scrollToEventId?: string;
  /** File path to pre-select as facet filter on mount. */
  initialFilePath?: string;
}

export function SessionTimeline({ sessionId, scrollToEventId, initialFilePath }: SessionTimelineProps) {
  // The tool-call waterfall is a wall (Max: too busy) — folded by default.
  const [showTrace, setShowTrace] = usePersistedFlag("os.events.trace", false);
  // The same category filters as the Live feed (Max's ask) — one language.
  const [activeFilter, setActiveFilter] = useState("all");
  // The turns+facets rail is dense (Max & Katie: too much) — folded by default.
  const [showRail, setShowRail] = usePersistedFlag("os.events.rail", false);
  const { records: rawRecords, loading, capped } = useSessionRecords(sessionId);
  // The shared cache holds the raw truth; noise-filtering is this view's own lens.
  const records = useMemo(() => filterNoise(rawRecords), [rawRecords]);
  // toolcall↔result: each record's round-trip partner, for the ⇄ jump.
  const pairMap = useMemo(() => toolPairMap(rawRecords), [rawRecords]);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  // Facet state
  const [selectedTurn, setSelectedTurn] = useState<number | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [selectedTool, setSelectedTool] = useState<string | null>(null);
  const [selectedPlan, setSelectedPlan] = useState<string | null>(null);

  // Reset when switching sessions
  useEffect(() => {
    setSelectedTurn(null);
    setSelectedFile(null);
    setSelectedTool(null);
    setSelectedPlan(null);
    setExpandedIds(new Set());
  }, [sessionId]);

  // Apply initial file path as facet filter once records are loaded
  const appliedInitialFile = useRef(false);
  useEffect(() => {
    if (initialFilePath && records.length > 0 && !appliedInitialFile.current) {
      appliedInitialFile.current = true;
      setSelectedFile(initialFilePath);
    }
  }, [initialFilePath, records]);

  // Reset applied flag when session changes
  useEffect(() => {
    appliedInitialFile.current = false;
  }, [sessionId]);

  // Build graph
  const graph = useMemo(() => buildEventGraph(records), [records]);
  const files = useMemo(() => fileFacets(graph, records), [graph, records]);
  const tools = useMemo(() => toolFacets(graph), [graph]);
  const plans = useMemo(() => planFacets(graph), [graph]);

  // Apply facets
  const facets: ActiveFacets = useMemo(() => ({
    ...(selectedTurn != null ? { turn: selectedTurn } : {}),
    ...(selectedFile != null ? { file: selectedFile } : {}),
    ...(selectedTool != null ? { tool: selectedTool } : {}),
    ...(selectedPlan != null ? { plan: selectedPlan } : {}),
  }), [selectedTurn, selectedFile, selectedTool, selectedPlan]);

  const matchedIds = useMemo(
    () => new Set(applyFacets(graph, records, facets)),
    [graph, records, facets],
  );

  const filteredRecords = useMemo(
    () => records.filter((r) => matchedIds.has(r.id)),
    [records, matchedIds],
  );

  const categoryFiltered = useMemo(() => {
    if (activeFilter === "all") return filteredRecords;
    const predicate = TIMELINE_FILTERS[activeFilter] ?? TIMELINE_FILTERS["all"]!;
    return filteredRecords.filter(predicate);
  }, [filteredRecords, activeFilter]);
  const rows = useMemo(() => toTimelineRows(categoryFiltered), [categoryFiltered]);

  const hasFacets = selectedTurn != null || selectedFile != null || selectedTool != null || selectedPlan != null;

  // Virtualized event list: a 100k-event session must render a few dozen
  // DOM rows, not 85k (measured pre-fix: 983 MB heap / 34 s). Dynamic
  // heights via measureElement — cards expand and collapse.
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollContainerRef.current,
    estimateSize: () => 44,
    overscan: 12,
  });

  // Keyboard navigation
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const selectedIndexRef = useRef(selectedIndex);
  selectedIndexRef.current = selectedIndex;
  const [eventsFocused, setEventsFocused] = useState(false);
  const exploreSidebarRef = useRef<HTMLDivElement>(null);

  // Clear selection on facet/data changes
  useEffect(() => { setSelectedIndex(null); }, [rows]);

  // Event list keyboard handler: up/down to navigate, left to jump to sidebar
  useEffect(() => {
    const el = scrollContainerRef.current;
    if (!el) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        exploreSidebarRef.current?.focus();
        return;
      }
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
      e.preventDefault();
      const direction = e.key === "ArrowDown" ? "down" : "up";
      const next = nextCardIndex(rows, selectedIndexRef.current, direction);
      if (next === null || next === selectedIndexRef.current) return;
      setSelectedIndex(next);
      rowVirtualizer.scrollToIndex(next, { align: "auto" });
    };

    el.addEventListener("keydown", onKeyDown);
    return () => el.removeEventListener("keydown", onKeyDown);
  }, [rows]);

  // Explore sidebar keyboard handler: right arrow to jump to event list
  useEffect(() => {
    const el = exploreSidebarRef.current;
    if (!el) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight") {
        e.preventDefault();
        scrollContainerRef.current?.focus();
        return;
      }
    };

    el.addEventListener("keydown", onKeyDown);
    return () => el.removeEventListener("keydown", onKeyDown);
  }, []);

  // Jump from a ribbon mark to its event card: expand + select + scroll.
  const selectEvent = useCallback((id: string) => {
    setExpandedIds((prev) => new Set([...prev, id]));
    const idx = rows.findIndex((r) => r.id === id);
    if (idx >= 0) {
      setSelectedIndex(idx);
      rowVirtualizer.scrollToIndex(idx, { align: "center" });
    }
  }, [rows, rowVirtualizer]);

  const selectedEventId = selectedIndex != null ? rows[selectedIndex]?.id ?? null : null;

  // Jump from a trace span (call_id) to its tool_call event card.
  const selectSpan = useCallback((callId: string) => {
    const target = records.find(
      (r) => r.record_type === "tool_call" && (r.payload as { call_id?: string })?.call_id === callId,
    );
    if (target) selectEvent(target.id);
  }, [records, selectEvent]);

  const selectedCallId = useMemo(() => {
    if (selectedEventId == null) return null;
    const r = records.find((rec) => rec.id === selectedEventId);
    return r && r.record_type === "tool_call" ? (r.payload as { call_id?: string })?.call_id ?? null : null;
  }, [selectedEventId, records]);

  const toggleExpand = useCallback((id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const expandAll = useCallback(() => {
    setExpandedIds(new Set(rows.map((r) => r.id)));
  }, [rows]);

  const collapseAll = useCallback(() => {
    setExpandedIds(new Set());
  }, []);

  const clearFacets = useCallback(() => {
    setSelectedTurn(null);
    setSelectedFile(null);
    setSelectedTool(null);
    setSelectedPlan(null);
  }, []);

  // Scroll to target event after rows render
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!scrollToEventId || rows.length === 0) return;
    // Expand the target event and select it
    setExpandedIds((prev) => new Set([...prev, scrollToEventId]));
    const targetIndex = rows.findIndex((r) => r.id === scrollToEventId);
    if (targetIndex >= 0) {
      setSelectedIndex(targetIndex);
      // The target may not be in the DOM yet (virtualized) — scroll by index.
      requestAnimationFrame(() => {
        // Two scrolls: the PAGE to the timeline region (the busy wall above
        // otherwise hides the whole list below the fold), then the virtual
        // list to the card. Without the first, "focused" was invisible.
        scrollContainerRef.current?.scrollIntoView({ block: "start", behavior: "smooth" });
        rowVirtualizer.scrollToIndex(targetIndex, { align: "center" });
        scrollContainerRef.current?.focus({ preventScroll: true });
      });
    }
  }, [scrollToEventId, rows, rowVirtualizer]);

  if (loading) {
    return (
      <div className="flex min-h-0 flex-1" data-testid="session-timeline">
        <div className="w-52 shrink-0 border-r border-[color:var(--divider)] bg-[color:var(--bg)]" />
        <div className="min-w-0 flex-1">
          <SessionVizSkeleton />
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0" data-testid="session-timeline">
      {/* Navigation sidebar: turns + facets — folded by default into a slim
          labeled rail; expands on demand, remembers the choice. */}
      {!showRail && (
        <div className="flex shrink-0 flex-col border-r border-[color:var(--divider)] bg-[color:var(--bg)] px-1.5 pt-3">
          <button
            onClick={() => setShowRail(true)}
            className="rounded-lg border border-[color:var(--border)] px-2 py-2 text-[length:var(--fs-label)] font-medium text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)] [writing-mode:vertical-rl]"
            title="Show turns and facets"
          >
            ▸ turns
          </button>
        </div>
      )}
      {showRail && (
      <div className="w-52 shrink-0 border-r border-[color:var(--divider)] overflow-y-auto bg-[color:var(--bg)] outline-none" ref={exploreSidebarRef} tabIndex={0}>
        <div className="flex justify-end px-2 pt-2">
          <button
            onClick={() => setShowRail(false)}
            className="rounded border border-[color:var(--border)] px-2 py-0.5 text-[length:var(--fs-label)] text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
            title="Collapse turns and facets"
          >
            ◂ hide
          </button>
        </div>
        <TurnOutline
          turns={graph.turns}
          selectedTurn={selectedTurn}
          onSelectTurn={setSelectedTurn}
        />
        <FacetPanel
          files={files}
          tools={tools}
          plans={plans}
          selectedFile={selectedFile}
          selectedTool={selectedTool}
          selectedPlan={selectedPlan}
          onSelectFile={setSelectedFile}
          onSelectTool={setSelectedTool}
          onSelectPlan={setSelectedPlan}
        />
      </div>
      )}

      {/* Event cards */}
      <div className="flex min-h-0 flex-1 min-w-0 flex-col outline-none" tabIndex={0} onFocus={() => setEventsFocused(true)} onBlur={() => setEventsFocused(false)}>
        {/* Shared summary header — the one-product spine (clickable stats) */}
        <div className="border-b border-[color:var(--divider)] bg-[color:var(--bg-surface)]">
          <SessionSummaryHeader
            records={records}
            onJumpToError={() => {
              const id = firstErrorEventId(records);
              if (id) selectEvent(id);
            }}
            onFilterFile={(path) => setSelectedFile(path)}
          />
        </div>

        {/* Activity ribbon — temporal shape of the whole session */}
        <div className="border-b border-[color:var(--divider)] bg-[color:var(--bg)]">
          <SessionActivityRibbon
            records={records}
            selectedEventId={selectedEventId}
            onSelectEvent={selectEvent}
          />
          <div className="border-t border-[color:var(--divider)]">
            <div className="flex items-center px-3 py-1.5">
              <button
                onClick={() => setShowTrace(!showTrace)}
                className="rounded border border-[color:var(--border)] px-2 py-0.5 text-[length:var(--fs-label)] text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
                aria-expanded={showTrace}
                title={showTrace ? "Hide the tool-call waterfall" : "Show every tool call with durations"}
              >
                {showTrace ? "▾ hide tool trace" : "▸ tool trace"}
              </button>
            </div>
            {showTrace && (
              <TurnTraceView records={records} onSelectSpan={selectSpan} selectedCallId={selectedCallId} />
            )}
          </div>
        </div>

        {/* Category filters — the Live feed's pills, same language here. */}
        <div className="flex flex-wrap items-center gap-1 border-b border-[color:var(--divider)] bg-[color:var(--bg-surface)] px-3 py-1.5">
          {FILTER_GROUPS.flatMap((g) => g.filters).map((f) => (
            <button
              key={f}
              onClick={() => setActiveFilter(f)}
              className={
                activeFilter === f
                  ? "rounded bg-[color:var(--accent)] px-2 py-0.5 text-[length:var(--fs-label)] font-medium text-[color:var(--bg-surface)]"
                  : "rounded px-2 py-0.5 text-[length:var(--fs-label)] text-[color:var(--text-muted)] transition-colors hover:bg-[color:var(--bg-hover)]/40 hover:text-[color:var(--text)]"
              }
            >
              {FILTER_LABELS[f] ?? f}
            </button>
          ))}
        </div>

        {/* Toolbar */}
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[color:var(--divider)] text-[10px] text-[color:var(--text-muted)]">
          <span>
            {hasFacets
              ? `${filteredRecords.length} of ${records.length} events`
              : `${records.length} events`}
          </span>
          {hasFacets && (
            <button
              onClick={clearFacets}
              className="text-[color:var(--accent)] hover:text-[color:var(--accent)]"
            >
              Clear filters
            </button>
          )}
          <span className="ml-auto flex items-center gap-2">
            <button onClick={expandAll} className="hover:text-[color:var(--text)]">Expand all</button>
            <span className="text-[color:var(--bg-hover)]">|</span>
            <button onClick={collapseAll} className="hover:text-[color:var(--text)]">Collapse all</button>
          </span>
        </div>

        {/* Event list — virtualized; only the viewport's rows exist in the DOM.
            The scroll element wraps ONLY the sized list (same structure as
            ConversationView) so the virtualizer's rect is the viewport, not
            the content. Header/ribbon/toolbar pin above it. */}
        {capped && (
          <div className="border-b border-[color:var(--orange)]/30 bg-[color:var(--orange)]/10 px-3 py-1 text-[10px] text-[color:var(--orange)]">
            Large session — showing the most recent {rows.length.toLocaleString()} events; older history is not loaded.
          </div>
        )}
        <div className="h-[70vh] min-h-[320px] overflow-y-auto" ref={scrollContainerRef}>
        <div className="relative" style={{ height: rows.length === 0 ? undefined : rowVirtualizer.getTotalSize() }}>
          {rows.length === 0 ? (
            <div className="p-4 text-xs text-[color:var(--text-muted)] text-center">
              No events match the selected filters
            </div>
          ) : (
            rowVirtualizer.getVirtualItems().map((vi) => {
              const row = rows[vi.index]!;
              const i = vi.index;
              return (
                <div
                  key={row.id}
                  data-os-target="event"
                  data-os-id={row.id}
                  data-event-id={row.id}
                  data-index={vi.index}
                  ref={rowVirtualizer.measureElement}
                  className="absolute left-0 top-0 w-full"
                  style={{ transform: `translateY(${vi.start}px)` }}
                >
                  <EventCardRow
                    row={row}
                    compact={!expandedIds.has(row.id)}
                    selected={eventsFocused && selectedIndex === i}
                    onClick={() => { toggleExpand(row.id); setSelectedIndex(i); scrollContainerRef.current?.focus(); }}
                    pairedEventId={pairMap.get(row.id)}
                  />
                </div>
              );
            })
          )}
        </div>
        </div>
      </div>
    </div>
  );
}
