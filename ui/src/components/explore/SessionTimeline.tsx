/** Session event timeline with faceted graph navigation.
 *  Left: turn outline + file/tool facets. Right: event cards (compact, click to expand). */

import { useState, useEffect, useRef, useMemo, useCallback } from "react";
import type { WireRecord } from "@/types/wire-record";
import { toTimelineRows } from "@/lib/timeline";
import { filterNoise } from "@/lib/explore-filters";
import { buildEventGraph, applyFacets, fileFacets, toolFacets, planFacets, type ActiveFacets } from "@/lib/event-graph";
import { TurnOutline } from "./TurnOutline";
import { FacetPanel } from "./FacetPanel";
import { EventCardRow } from "@/components/events/EventCard";
import { nextCardIndex } from "@/lib/keyboard-nav";

interface SessionTimelineProps {
  sessionId: string;
  /** Event ID to scroll into view after records load. */
  scrollToEventId?: string;
  /** File path to pre-select as facet filter on mount. */
  initialFilePath?: string;
}

export function SessionTimeline({ sessionId, scrollToEventId, initialFilePath }: SessionTimelineProps) {
  const [records, setRecords] = useState<WireRecord[]>([]);
  const [loading, setLoading] = useState(true);
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

  // Fetch records
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setRecords([]);

    fetch(`/api/sessions/${sessionId}/records`)
      .then((r) => r.json())
      .then((data: WireRecord[]) => {
        if (!cancelled) {
          setRecords(filterNoise(Array.isArray(data) ? data : []));
          setLoading(false);
        }
      })
      .catch(() => {
        if (!cancelled) setLoading(false);
      });

    return () => { cancelled = true; };
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

  const rows = useMemo(() => toTimelineRows(filteredRecords), [filteredRecords]);

  const hasFacets = selectedTurn != null || selectedFile != null || selectedTool != null || selectedPlan != null;

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
      requestAnimationFrame(() => {
        const row = rows[next];
        if (!row) return;
        const card = el.querySelector(`[data-event-id="${CSS.escape(row.id)}"]`);
        card?.scrollIntoView({ behavior: "smooth", block: "nearest" });
      });
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
    if (targetIndex >= 0) setSelectedIndex(targetIndex);
    // Double-rAF: first frame expands the card, second frame scrolls after layout
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const el = scrollContainerRef.current?.querySelector(`[data-event-id="${CSS.escape(scrollToEventId)}"]`);
        el?.scrollIntoView({ behavior: "smooth", block: "center" });
        scrollContainerRef.current?.focus();
      });
    });
  }, [scrollToEventId, rows]);

  if (loading) {
    return <div className="p-4 text-xs text-[#565f89]">Loading events...</div>;
  }

  return (
    <div className="flex min-h-0" data-testid="session-timeline">
      {/* Navigation sidebar: turns + facets */}
      <div className="w-52 shrink-0 border-r border-[#2f3348] overflow-y-auto bg-[#1a1b26] outline-none" ref={exploreSidebarRef} tabIndex={0}>
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

      {/* Event cards */}
      <div className="flex-1 min-w-0 overflow-y-auto outline-none" ref={scrollContainerRef} tabIndex={0} onFocus={() => setEventsFocused(true)} onBlur={() => setEventsFocused(false)}>
        {/* Toolbar */}
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[#2f3348] text-[10px] text-[#565f89]">
          <span>
            {hasFacets
              ? `${filteredRecords.length} of ${records.length} events`
              : `${records.length} events`}
          </span>
          {hasFacets && (
            <button
              onClick={clearFacets}
              className="text-[#7aa2f7] hover:text-[#89b4fa]"
            >
              Clear filters
            </button>
          )}
          <span className="ml-auto flex items-center gap-2">
            <button onClick={expandAll} className="hover:text-[#c0caf5]">Expand all</button>
            <span className="text-[#2f3348]">|</span>
            <button onClick={collapseAll} className="hover:text-[#c0caf5]">Collapse all</button>
          </span>
        </div>

        {/* Event list */}
        <div className="py-1">
          {rows.length === 0 ? (
            <div className="p-4 text-xs text-[#565f89] text-center">
              No events match the selected filters
            </div>
          ) : (
            rows.map((row, i) => (
              <div key={row.id} data-event-id={row.id}>
                <EventCardRow
                  row={row}
                  compact={!expandedIds.has(row.id)}
                  selected={eventsFocused && selectedIndex === i}
                  onClick={() => { toggleExpand(row.id); setSelectedIndex(i); scrollContainerRef.current?.focus(); }}
                />
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
