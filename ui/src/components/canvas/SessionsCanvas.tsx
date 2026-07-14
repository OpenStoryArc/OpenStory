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
import { useResizablePanel } from "@/hooks/use-resizable-panel";
import { isSubagentSession } from "@/lib/subagents";
import { buildHierarchy, type GroupDim, type HNode } from "@/lib/sessions-canvas";
import { fitTransform } from "@/lib/canvas-fit";
import { CANVAS_MODES, MODE_META, modeUsesGroupBy, type CanvasMode, DEFAULT_CANVAS_MODE } from "@/lib/canvas-modes";
import { controlActions$ } from "@/streams/control";
import { postInteraction, selectInteraction } from "@/lib/interaction";
import type { Metric } from "@/lib/session-hierarchy-tree";
import { sessionColor } from "@/lib/session-colors";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { SessionVizLoader } from "@/components/viz/SessionVizLoader";
import { SpaceFillingView } from "./SpaceFillingView";
import { GanttView } from "./GanttView";
import { ScatterView } from "./ScatterView";
import { ToolFlowView } from "./ToolFlowView";
import { ToolAdjacencyHeatmap } from "@/components/canvas/ToolAdjacencyHeatmap";
import { HeatmapView } from "@/components/canvas/HeatmapView";
import { AgentProjectMatrix } from "@/components/canvas/AgentProjectMatrix";
import { DurationBeeswarm } from "@/components/canvas/DurationBeeswarm";
import { cn } from "@/lib/cn";

/** Canvas view modes are exactly the shared CANVAS_MODES — binding the local
 *  alias to the exported union keeps MODE_CAPTION completeness tsc-enforced. */
type ViewMode = CanvasMode;

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
  "tool-adjacency": () => `From × to heatmap of tool transitions across sampled sessions · brighter = that pair fires more often · the diagonal is a tool repeating itself.`,
  "agent-project": () => `Rows = agents, columns = projects · brighter cell = more events there · the block structure shows which agent owns which project.`,
  heatmap: () => `Contribution calendar in 3D · each day a stack, each box a session (warm base = biggest) · hover to see a session, click to open it · 2D toggle inside.`,
  durations: () => `One dot = a session · x = duration on a log scale · lane + color = agent · the swarm shows each agent's spread of session lengths and its outliers.`,
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
  const [viewMode, setViewMode] = useState<ViewMode>(DEFAULT_CANVAS_MODE);
  const [metric, setMetric] = useState<Metric>("events");
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<{ sessionId: string; label: string } | null>(null);
  const panel = useResizablePanel("canvas.detail.width", 420, 320, 760);
  const [query, setQuery] = useState("");
  const nowMs = useMemo(() => Date.now(), []);

  // Agent-in-UI: apply `canvas.*` toggle intents (component-local state that
  // isn't in the URL). The Canvas is a sink — it reacts to control$, validating
  // each value before applying so the vocabulary stays open but safe.
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type !== "toggle") return;
      if (a.target === "canvas.mode" && (CANVAS_MODES as readonly string[]).includes(a.value)) {
        setViewMode(a.value as ViewMode);
      } else if (a.target === "canvas.groupBy" && DIMS.some((d) => d.key === a.value)) {
        setGroupBy(a.value as GroupDim);
      } else if (a.target === "canvas.metric" && (a.value === "events" || a.value === "tokens")) {
        setMetric(a.value as Metric);
      }
    });
    return () => sub.unsubscribe();
  }, []);

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
    // click-granular capture: record WHICH session the user clicked into on the
    // Canvas (every mode routes through here), so the journey is replayable.
    postInteraction(selectInteraction("canvas", id));
  };

  const q = query.trim().toLowerCase();
  const dim = (n: HNode) => Boolean(q) && !`${n.label} ${n.sessionId ?? ""}`.toLowerCase().includes(q);

  const groupColor = (n: HNode) => (n.status === "ongoing" ? "#9ece6a" : sessionColor(n.sessionId ?? n.label));

  const usesGroupBy = modeUsesGroupBy(viewMode);
  const caption = MODE_CAPTION[viewMode](groupBy, metric);

  return (
    <div className="flex min-h-0 flex-1 bg-[color:var(--bg)] text-[color:var(--text)]" data-testid="sessions-canvas">
      <div className="flex min-w-0 flex-1 flex-col">
        {/* toolbar */}
        <div className="flex flex-wrap items-center gap-2 border-b border-[color:var(--bg-hover)] bg-[color:var(--bg)] px-3 py-2">
          <div className="flex flex-wrap rounded border border-[color:var(--border)] p-0.5">
            {CANVAS_MODES.map((m) => (
              <button
                key={m}
                onClick={() => setViewMode(m)}
                title={MODE_META[m].blurb}
                className={cn("flex items-center gap-1 rounded px-2 py-0.5 text-[11px] transition-colors", viewMode === m ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]")}
              >
                <span aria-hidden className="text-[10px] opacity-80">{MODE_META[m].icon}</span>{MODE_META[m].label}
              </button>
            ))}
          </div>
          {(viewMode === "sunburst" || viewMode === "treemap") && (
            <div className="flex rounded border border-[color:var(--border)] p-0.5">
              {(["events", "tokens"] as Metric[]).map((mt) => (
                <button key={mt} onClick={() => setMetric(mt)} className={cn("rounded px-2 py-0.5 text-[11px] transition-colors", metric === mt ? "bg-[color:var(--orange)] text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]")}>{mt}</button>
              ))}
            </div>
          )}
          {usesGroupBy && (
            <>
              <span className="ml-1 text-[10px] uppercase tracking-wide text-[color:var(--text-muted)]">group by</span>
              <div className="flex flex-wrap gap-1">
                {DIMS.map((d) => (
                  <button
                    key={d.key}
                    onClick={() => { setGroupBy(d.key); setExpanded(new Set()); setSelected(null); }}
                    className={cn("rounded px-1.5 py-0.5 text-[11px] transition-colors", groupBy === d.key ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:bg-[color:var(--bg-hover)] hover:text-[color:var(--text)]")}
                  >
                    {d.label}
                  </button>
                ))}
              </div>
            </>
          )}
          {!usesGroupBy && MODE_META[viewMode].groupByNote && (
            <span className="ml-1 text-[10px] italic text-[color:var(--text-muted)]">{MODE_META[viewMode].groupByNote}</span>
          )}
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search…" className="ml-2 w-40 rounded border border-[color:var(--bg-hover)] bg-[color:var(--bg-surface)] px-2 py-1 text-[12px] text-[color:var(--text)] placeholder:text-[color:var(--text-muted)] focus:border-[color:var(--accent)] focus:outline-none" />
          {viewMode === "board" && (
            <>
              <button onClick={() => setExpanded(new Set())} className="rounded border border-[color:var(--border)] px-2 py-1 text-[11px] text-[color:var(--text-muted)] hover:text-[color:var(--text)]">Collapse all</button>
              <button onClick={fit} className="rounded border border-[color:var(--border)] px-2 py-1 text-[11px] text-[color:var(--text-muted)] hover:text-[color:var(--text)]">Fit</button>
            </>
          )}
        </div>
        {/* per-mode caption — what am I looking at + the encoding */}
        <div className="border-b border-[color:var(--bg-hover)]/60 bg-[color:var(--bg)] px-3 py-1 text-[10px] text-[color:var(--text-muted)]">{caption}</div>

        <div ref={wrapRef} className="relative min-h-0 flex-1">
          {loading && <div className="absolute inset-0 flex items-center justify-center text-[12px] text-[color:var(--text-muted)]">Loading canvas…</div>}
          {viewMode === "gantt" ? (
            <GanttView sessions={universe} groupBy={groupBy} width={size.w} height={size.h} nowMs={nowMs} onOpenSession={openSessionPanel} />
          ) : viewMode === "scatter" ? (
            <ScatterView sessions={universe} width={size.w} height={size.h} onOpenSession={openSessionPanel} />
          ) : viewMode === "flow" ? (
            <ToolFlowView sessions={universe} width={size.w} height={size.h} onOpenSession={openSessionPanel} />
          ) : viewMode === "tool-adjacency" ? (
            <div className="absolute inset-0 overflow-auto"><ToolAdjacencyHeatmap sessions={universe} /></div>
          ) : viewMode === "agent-project" ? (
            <div className="absolute inset-0 overflow-auto"><AgentProjectMatrix
              sessions={universe}
              onOpenCell={(agent, project) =>
                onNavigate({
                  view: "explore",
                  explore: { filters: { agent, ...(project !== "other" && project !== "unknown" ? { project } : {}) } },
                })
              }
            /></div>
          ) : viewMode === "heatmap" ? (
            <div className="absolute inset-0 flex flex-col overflow-auto"><HeatmapView onNavigate={onNavigate} onOpenSession={openSessionPanel} /></div>
          ) : viewMode === "durations" ? (
            <div className="absolute inset-0 overflow-auto"><DurationBeeswarm sessions={universe} onOpenSession={openSessionPanel} /></div>
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

      {/* details side panel — user-resizable via the left-edge grip */}
      {selected && (
        <aside
          className="relative flex shrink-0 flex-col border-l border-[color:var(--bg-hover)] bg-[color:var(--bg)]"
          style={{ width: panel.width }}
        >
          {/* drag handle: a hit-target straddling the left border */}
          <div
            onPointerDown={panel.onHandlePointerDown}
            className={`absolute -left-1 top-0 z-10 h-full w-2 cursor-col-resize transition-colors hover:bg-[color:var(--accent)]/40 ${panel.dragging ? "bg-[color:var(--accent)]/60" : "bg-transparent"}`}
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize panel"
            title="Drag to resize"
          />
          <div className="flex items-center justify-between border-b border-[color:var(--bg-hover)] px-3 py-2">
            <span className="truncate text-[12px] text-[color:var(--text)]">{cleanHarnessPreview(selected.label).slice(0, 40)}</span>
            <div className="flex items-center gap-2">
              <button onClick={() => onNavigate({ view: "story", sessionId: selected.sessionId })} className="rounded px-2 py-0.5 text-[11px] text-[color:var(--accent)] hover:bg-[color:var(--bg-hover)]">Story →</button>
              <button onClick={() => onNavigate({ view: "explore", sessionId: selected.sessionId })} className="rounded px-2 py-0.5 text-[11px] text-[color:var(--accent)] hover:bg-[color:var(--bg-hover)]">Explore →</button>
              <button onClick={() => setSelected(null)} className="rounded px-1.5 text-[color:var(--text-muted)] hover:text-[color:var(--text)]" aria-label="Close">✕</button>
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
