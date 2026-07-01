/** SessionsCanvas — a Figma-like infinite board of session activity as a
 *  COLLAPSIBLE HIERARCHY (Group → Project → Session).
 *
 *  Starts at a handful of top-level groups (chosen via the group-by selector:
 *  day / user / agent / status / host / project) so it's not overwhelming; click
 *  a group or project to drill deeper; click a session to open a details side
 *  panel. Pan/zoom dot-grid canvas, persistent nodes, deterministic phyllotaxis
 *  layout (lib/sessions-canvas). */

import { useEffect, useMemo, useRef, useState } from "react";
import { select } from "d3-selection";
import { zoom as d3zoom, zoomIdentity, type ZoomBehavior } from "d3-zoom";
import { scaleSqrt } from "d3-scale";
import type { HashRoute } from "@/lib/hash-route";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { isSubagentSession } from "@/lib/subagents";
import { buildHierarchy, type GroupDim, type HNode } from "@/lib/sessions-canvas";
import { fitTransform } from "@/lib/canvas-fit";
import { CANVAS_MODES, MODE_META, modeUsesGroupBy } from "@/lib/canvas-modes";
import type { Metric } from "@/lib/session-hierarchy-tree";
import { sessionColor } from "@/lib/session-colors";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { SessionVizLoader } from "@/components/viz/SessionVizLoader";
import { SpaceFillingView } from "./SpaceFillingView";
import { GanttView } from "./GanttView";
import { ScatterView } from "./ScatterView";
import { ToolFlowView } from "./ToolFlowView";
import { cn } from "@/lib/cn";

type ViewMode = "board" | "sunburst" | "treemap" | "gantt" | "scatter" | "flow";

interface Props {
  onNavigate: (route: HashRoute) => void;
}

/** What-am-I-looking-at caption per mode (labels the chart + its encoding). */
const MODE_CAPTION: Record<ViewMode, (g: GroupDim, m: Metric) => string> = {
  board: (g) => `Sessions grouped by ${g}, then project — click a circle to expand into sessions, click a session for details.`,
  sunburst: (g, m) => `Ring = ${g} → project → session · angle = ${m}. Click a wedge to zoom in; the center to zoom out.`,
  treemap: (g, m) => `Area = ${m}, nested ${g} → project → session. Click a cell to zoom; use the breadcrumb to ascend.`,
  gantt: (g) => `Each bar = a session over time, lane-packed by ${g} · length = duration, color = agent, ongoing pulses. Drag the overview strip to window.`,
  scatter: () => `Each dot = a session · x = events, y = output-tokens (log-log) · the line is expected output — dots above it are more productive · size = duration, color = agent.`,
  flow: () => `How often one tool follows another for the chosen agent · ribbon width = number of transitions · left = the tool used, right = what came next.`,
};

const DIMS: { key: GroupDim; label: string }[] = [
  { key: "day", label: "Day (latest)" },
  { key: "user", label: "User" },
  { key: "agent", label: "Agent" },
  { key: "status", label: "Status" },
  { key: "host", label: "Host" },
  { key: "project", label: "Project" },
];

export function SessionsCanvas({ onNavigate }: Props) {
  const { sessions, loading } = useSessionsList();
  const [groupBy, setGroupBy] = useState<GroupDim>("user");
  const [viewMode, setViewMode] = useState<ViewMode>("board");
  const [metric, setMetric] = useState<Metric>("events");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<{ sessionId: string; label: string } | null>(null);
  const [query, setQuery] = useState("");
  const nowMs = useMemo(() => Date.now(), []);

  const wrapRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const zoomRef = useRef<ZoomBehavior<SVGSVGElement, unknown> | null>(null);
  const [size, setSize] = useState({ w: 1000, h: 700 });
  const [t, setT] = useState({ k: 1, x: 0, y: 0 });

  const universe = useMemo(() => sessions.filter((s) => !isSubagentSession(s.session_id)), [sessions]);
  const model = useMemo(() => buildHierarchy(universe, groupBy, expanded), [universe, groupBy, expanded]);
  const nodeByKey = useMemo(() => new Map(model.nodes.map((n) => [n.key, n])), [model]);

  const rScale = useMemo(() => {
    const mk = (kind: HNode["kind"], range: [number, number]) => {
      const vals = model.nodes.filter((n) => n.kind === kind).map((n) => (kind === "session" ? n.events : n.count));
      return scaleSqrt().domain([kind === "session" ? 0 : 1, Math.max(1, ...vals)]).range(range).clamp(true);
    };
    return { group: mk("group", [22, 74]), project: mk("project", [12, 42]), session: mk("session", [5, 18]) };
  }, [model]);
  const radius = (n: HNode) => rScale[n.kind](n.kind === "session" ? n.events : n.count);

  // measure
  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const measure = () => setSize({ w: el.clientWidth || 1000, h: el.clientHeight || 700 });
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // zoom/pan
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const sel = select(svg);
    const z = d3zoom<SVGSVGElement, unknown>().scaleExtent([0.08, 4]).on("zoom", (e) => setT({ k: e.transform.k, x: e.transform.x, y: e.transform.y }));
    zoomRef.current = z;
    sel.call(z);
    return () => { sel.on(".zoom", null); };
  }, []);

  const fit = () => {
    const svg = svgRef.current, z = zoomRef.current;
    if (!svg || !z) return;
    const { k, x, y } = fitTransform(model.bounds, size);
    select(svg).transition().duration(400).call(z.transform, zoomIdentity.translate(x, y).scale(k));
  };
  // fit on group-by change (not on every expand — keep the viewport stable while drilling)
  useEffect(() => { if (model.nodes.length && size.w > 100) fit(); }, [groupBy, size.w]); // eslint-disable-line react-hooks/exhaustive-deps
  // Initial fit once the data has actually arrived. Sessions load async, often
  // AFTER the viewport is measured, so the group-by/size effect above fires with
  // an empty model and never re-fits — the board loaded clipped. Fit exactly
  // once when both size and nodes are ready.
  const didInitialFit = useRef(false);
  useEffect(() => {
    if (didInitialFit.current) return;
    if (model.nodes.length && size.w > 100) { fit(); didInitialFit.current = true; }
  }, [model.nodes.length, size.w]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggle = (key: string) => setExpanded((p) => { const n = new Set(p); n.has(key) ? n.delete(key) : n.add(key); return n; });
  const onNodeClick = (n: HNode) => {
    if (n.kind === "session" && n.sessionId) setSelected({ sessionId: n.sessionId, label: n.label });
    else toggle(n.key);
  };
  const openSessionPanel = (id: string) => {
    const s = universe.find((x) => x.session_id === id);
    setSelected({ sessionId: id, label: s?.label || id.slice(0, 8) });
  };

  const q = query.trim().toLowerCase();
  const dim = (n: HNode) => Boolean(q) && !`${n.label} ${n.sessionId ?? ""}`.toLowerCase().includes(q);

  const groupColor = (n: HNode) => (n.status === "ongoing" ? "#9ece6a" : sessionColor(n.sessionId ?? n.label));

  const usesGroupBy = modeUsesGroupBy(viewMode);
  const caption = MODE_CAPTION[viewMode](groupBy, metric);

  return (
    <div className="flex min-h-0 flex-1 bg-[#16171f] text-[#c0caf5]" data-testid="sessions-canvas">
      <div className="flex min-w-0 flex-1 flex-col">
        {/* toolbar */}
        <div className="flex items-center gap-2 border-b border-[#2f3348] bg-[#1a1b26] px-3 py-2">
          <div className="flex rounded border border-[#3b4261] p-0.5">
            {CANVAS_MODES.map((m) => (
              <button
                key={m}
                onClick={() => setViewMode(m)}
                title={MODE_META[m].blurb}
                className={cn("flex items-center gap-1 rounded px-2 py-0.5 text-[11px] transition-colors", viewMode === m ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#565f89] hover:text-[#c0caf5]")}
              >
                <span aria-hidden className="text-[10px] opacity-80">{MODE_META[m].icon}</span>{MODE_META[m].label}
              </button>
            ))}
          </div>
          {(viewMode === "sunburst" || viewMode === "treemap") && (
            <div className="flex rounded border border-[#3b4261] p-0.5">
              {(["events", "tokens"] as Metric[]).map((mt) => (
                <button key={mt} onClick={() => setMetric(mt)} className={cn("rounded px-2 py-0.5 text-[11px] transition-colors", metric === mt ? "bg-[#e0af68] text-[#1a1b26]" : "text-[#565f89] hover:text-[#c0caf5]")}>{mt}</button>
              ))}
            </div>
          )}
          {usesGroupBy && (
            <>
              <span className="ml-1 text-[10px] uppercase tracking-wide text-[#565f89]">group by</span>
              <div className="flex flex-wrap gap-1">
                {DIMS.map((d) => (
                  <button
                    key={d.key}
                    onClick={() => { setGroupBy(d.key); setExpanded(new Set()); setSelected(null); }}
                    className={cn("rounded px-1.5 py-0.5 text-[11px] transition-colors", groupBy === d.key ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#565f89] hover:bg-[#2f3348] hover:text-[#c0caf5]")}
                  >
                    {d.label}
                  </button>
                ))}
              </div>
            </>
          )}
          {!usesGroupBy && MODE_META[viewMode].groupByNote && (
            <span className="ml-1 text-[10px] italic text-[#565f89]">{MODE_META[viewMode].groupByNote}</span>
          )}
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search…" className="ml-2 w-40 rounded border border-[#2f3348] bg-[#24283b] px-2 py-1 text-[12px] text-[#c0caf5] placeholder:text-[#565f89] focus:border-[#7aa2f7] focus:outline-none" />
          {viewMode === "board" && (
            <>
              <button onClick={() => setExpanded(new Set())} className="rounded border border-[#3b4261] px-2 py-1 text-[11px] text-[#565f89] hover:text-[#c0caf5]">Collapse all</button>
              <button onClick={fit} className="rounded border border-[#3b4261] px-2 py-1 text-[11px] text-[#565f89] hover:text-[#c0caf5]">Fit</button>
            </>
          )}
        </div>
        {/* per-mode caption — what am I looking at + the encoding */}
        <div className="border-b border-[#2f3348]/60 bg-[#1a1b26] px-3 py-1 text-[10px] text-[#565f89]">{caption}</div>

        <div ref={wrapRef} className="relative min-h-0 flex-1">
          {loading && <div className="absolute inset-0 flex items-center justify-center text-[12px] text-[#565f89]">Loading canvas…</div>}
          {viewMode === "gantt" ? (
            <GanttView sessions={universe} groupBy={groupBy} width={size.w} height={size.h} nowMs={nowMs} onOpenSession={openSessionPanel} />
          ) : viewMode === "scatter" ? (
            <ScatterView sessions={universe} width={size.w} height={size.h} onOpenSession={openSessionPanel} />
          ) : viewMode === "flow" ? (
            <ToolFlowView sessions={universe} width={size.w} height={size.h} onOpenSession={openSessionPanel} />
          ) : viewMode !== "board" ? (
            <SpaceFillingView sessions={universe} groupBy={groupBy} metric={metric} mode={viewMode} width={size.w} height={size.h} onOpenSession={openSessionPanel} />
          ) : (
          <svg ref={svgRef} width={size.w} height={size.h} className="block cursor-grab active:cursor-grabbing" style={{ touchAction: "none" }}>
            <defs>
              <pattern id="canvas-dots" width="26" height="26" patternUnits="userSpaceOnUse"><circle cx="1" cy="1" r="1" fill="#2f3348" fillOpacity="0.45" /></pattern>
            </defs>
            <rect width={size.w} height={size.h} fill="url(#canvas-dots)" />
            <g transform={`translate(${t.x},${t.y}) scale(${t.k})`}>
              {/* edges */}
              {model.edges.map((e) => {
                const a = nodeByKey.get(e.from), b = nodeByKey.get(e.to);
                if (!a || !b) return null;
                const dx = b.x - a.x, dy = b.y - a.y;
                const cx = (a.x + b.x) / 2 - dy * 0.12, cy = (a.y + b.y) / 2 + dx * 0.12;
                return <path key={`${e.from}-${e.to}`} d={`M${a.x},${a.y} Q${cx},${cy} ${b.x},${b.y}`} fill="none" stroke="#3b4261" strokeOpacity={0.5} strokeWidth={1.2} />;
              })}
              {/* nodes */}
              {model.nodes.map((n) => {
                const r = radius(n);
                const col = groupColor(n);
                const isSel = n.kind === "session" && selected?.sessionId === n.sessionId;
                const faded = dim(n);
                const drillable = n.kind !== "session" && n.collapsed;
                return (
                  <g key={n.key} data-canvas-node={n.key} data-kind={n.kind} transform={`translate(${n.x},${n.y})`} opacity={faded ? 0.15 : 1} className="cursor-pointer" onClick={() => onNodeClick(n)}>
                    <circle r={r + 4} fill={col} fillOpacity={0.12} />
                    <circle r={r} fill={col} fillOpacity={n.kind === "session" ? 0.8 : 0.9} stroke={isSel ? "#c0caf5" : "#16171f"} strokeWidth={isSel ? 2.5 : n.kind === "session" ? 1 : 2} />
                    {n.kind !== "session" && (
                      <text y={r < 16 ? 3 : 2} textAnchor="middle" fontSize={Math.min(13, Math.max(8, r / 1.7))} fontWeight={700} fill="#16171f">{n.count}</text>
                    )}
                    {n.kind !== "session" && (
                      <text y={r + 13} textAnchor="middle" fontSize={n.kind === "group" ? 12 : 10} fontWeight={n.kind === "group" ? 600 : 400} fill="#c0caf5">
                        {cleanHarnessPreview(String(n.label)).replace(/^-/, "").split(/[/]/).pop()?.slice(0, 22)}{drillable ? " +" : ""}
                      </text>
                    )}
                  </g>
                );
              })}
            </g>
          </svg>
          )}
        </div>
      </div>

      {/* details side panel */}
      {selected && (
        <aside className="flex w-[420px] shrink-0 flex-col border-l border-[#2f3348] bg-[#1a1b26]">
          <div className="flex items-center justify-between border-b border-[#2f3348] px-3 py-2">
            <span className="truncate text-[12px] text-[#c0caf5]">{cleanHarnessPreview(selected.label).slice(0, 40)}</span>
            <div className="flex items-center gap-2">
              <button onClick={() => onNavigate({ view: "story", sessionId: selected.sessionId })} className="rounded px-2 py-0.5 text-[11px] text-[#bb9af7] hover:bg-[#2f3348]">Story →</button>
              <button onClick={() => onNavigate({ view: "explore", sessionId: selected.sessionId })} className="rounded px-2 py-0.5 text-[11px] text-[#7aa2f7] hover:bg-[#2f3348]">Explore →</button>
              <button onClick={() => setSelected(null)} className="rounded px-1.5 text-[#565f89] hover:text-[#c0caf5]" aria-label="Close">✕</button>
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">
            <SessionVizLoader sessionId={selected.sessionId} onOpenSubagent={(id) => onNavigate({ view: "explore", sessionId: id })} />
          </div>
        </aside>
      )}
    </div>
  );
}
