/** SessionsCanvas — a Figma-like infinite board of session activity.
 *
 *  Sessions cluster by project into collapsible super-nodes (inspired by the
 *  religious-freedom concept graph's bounded contexts). Click a project to bloom
 *  it into its sessions; pan/zoom the dot-grid canvas; click a session to open
 *  it. Deterministic phyllotaxis layout (lib/sessions-canvas) + persistent
 *  D3-zoom'd nodes — no per-frame rebuild, no random layout. */

import { useEffect, useMemo, useRef, useState } from "react";
import { select } from "d3-selection";
import { zoom as d3zoom, zoomIdentity, type ZoomBehavior } from "d3-zoom";
import { scaleSqrt } from "d3-scale";
import type { HashRoute } from "@/lib/hash-route";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { isSubagentSession } from "@/lib/subagents";
import { buildCanvas } from "@/lib/sessions-canvas";
import { projectColor } from "@/lib/project-color";
import { sessionColor } from "@/lib/session-colors";
import { cleanHarnessPreview } from "@/lib/harness-message";

interface Props {
  onNavigate: (route: HashRoute) => void;
}

export function SessionsCanvas({ onNavigate }: Props) {
  const { sessions, loading } = useSessionsList();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const wrapRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const zoomRef = useRef<ZoomBehavior<SVGSVGElement, unknown> | null>(null);
  const [size, setSize] = useState({ w: 1000, h: 700 });
  const [t, setT] = useState({ k: 1, x: 0, y: 0 });
  const didFit = useRef(false);

  const universe = useMemo(() => sessions.filter((s) => !isSubagentSession(s.session_id)), [sessions]);
  const model = useMemo(() => buildCanvas(universe, expanded), [universe, expanded]);

  const clusterR = useMemo(() => {
    const maxC = Math.max(1, ...model.clusters.map((c) => c.count));
    return scaleSqrt().domain([1, maxC]).range([16, 66]).clamp(true);
  }, [model]);
  const nodeR = useMemo(() => {
    const maxE = Math.max(1, ...model.nodes.map((n) => n.events));
    return scaleSqrt().domain([0, maxE]).range([6, 22]).clamp(true);
  }, [model]);

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
    const z = d3zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.1, 4])
      .on("zoom", (e) => setT({ k: e.transform.k, x: e.transform.x, y: e.transform.y }));
    zoomRef.current = z;
    sel.call(z);
    return () => { sel.on(".zoom", null); };
  }, []);

  const fit = () => {
    const svg = svgRef.current, z = zoomRef.current;
    if (!svg || !z) return;
    const b = model.bounds;
    const pad = 90;
    const bw = Math.max(b.maxX - b.minX + pad * 2, 1);
    const bh = Math.max(b.maxY - b.minY + pad * 2, 1);
    const scale = Math.min(size.w / bw, size.h / bh, 1.4);
    const cx = (b.minX + b.maxX) / 2, cy = (b.minY + b.maxY) / 2;
    select(svg).transition().duration(400).call(z.transform, zoomIdentity.translate(size.w / 2, size.h / 2).scale(scale).translate(-cx, -cy));
  };
  // fit once on first data + on size
  useEffect(() => {
    if (!didFit.current && model.clusters.length > 0 && size.w > 100) { didFit.current = true; fit(); }
  }, [model, size]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggle = (project: string) =>
    setExpanded((prev) => { const n = new Set(prev); n.has(project) ? n.delete(project) : n.add(project); return n; });

  const q = query.trim().toLowerCase();
  const clusterDim = (project: string) => Boolean(q) && !project.toLowerCase().includes(q);
  const nodeDim = (label: string, project: string) => Boolean(q) && !(`${label} ${project}`.toLowerCase().includes(q));

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-[#16171f] text-[#c0caf5]" data-testid="sessions-canvas">
      <div className="flex items-center gap-3 border-b border-[#2f3348] bg-[#1a1b26] px-3 py-2">
        <span className="text-[11px] text-[#565f89]">{model.clusters.length} projects · {universe.length} sessions · {expanded.size} expanded</span>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search projects / sessions…"
          className="ml-2 w-56 rounded border border-[#2f3348] bg-[#24283b] px-2 py-1 text-[12px] text-[#c0caf5] placeholder:text-[#565f89] focus:border-[#7aa2f7] focus:outline-none"
        />
        <button onClick={() => setExpanded(new Set())} className="rounded border border-[#3b4261] px-2 py-1 text-[11px] text-[#565f89] hover:text-[#c0caf5]">Collapse all</button>
        <button onClick={fit} className="rounded border border-[#3b4261] px-2 py-1 text-[11px] text-[#565f89] hover:text-[#c0caf5]">Fit</button>
        <span className="ml-auto text-[10px] text-[#565f89]">scroll to zoom · drag to pan · click a project to expand</span>
      </div>

      <div ref={wrapRef} className="relative min-h-0 flex-1">
        {loading && <div className="absolute inset-0 flex items-center justify-center text-[12px] text-[#565f89]">Loading canvas…</div>}
        <svg ref={svgRef} width={size.w} height={size.h} className="block cursor-grab active:cursor-grabbing" style={{ touchAction: "none" }}>
          <defs>
            <pattern id="canvas-dots" width="26" height="26" patternUnits="userSpaceOnUse">
              <circle cx="1" cy="1" r="1" fill="#2f3348" fillOpacity="0.45" />
            </pattern>
          </defs>
          <rect width={size.w} height={size.h} fill="url(#canvas-dots)" />
          <g transform={`translate(${t.x},${t.y}) scale(${t.k})`}>
            {/* expanded project hulls */}
            {model.clusters.filter((c) => !c.collapsed).map((c) => {
              const ns = model.nodes.filter((n) => n.project === c.project);
              let r = 44;
              ns.forEach((n) => { r = Math.max(r, Math.hypot(n.x - c.x, n.y - c.y) + 30); });
              const col = projectColor(c.project);
              return (
                <g key={`hull-${c.project}`}>
                  <circle cx={c.x} cy={c.y} r={r} fill={col} fillOpacity={0.05} stroke={col} strokeOpacity={0.4} strokeDasharray="4 5" />
                  <text x={c.x} y={c.y - r + 14} textAnchor="middle" fontSize={12} fontWeight={600} fill={col} className="cursor-pointer" onClick={() => toggle(c.project)}>
                    {c.project} ⊖
                  </text>
                </g>
              );
            })}
            {/* session nodes (bloomed) */}
            {model.nodes.map((n) => {
              const r = nodeR(n.events);
              const col = n.status === "ongoing" ? "#9ece6a" : sessionColor(n.id);
              const dim = nodeDim(n.label, n.project);
              return (
                <g key={n.id} data-canvas-node={n.id} transform={`translate(${n.x},${n.y})`} opacity={dim ? 0.15 : 1} className="cursor-pointer" onClick={() => onNavigate({ view: "explore", sessionId: n.id })}>
                  <circle r={r} fill={col} fillOpacity={0.8} stroke={col} strokeWidth={1} />
                  <title>{`${cleanHarnessPreview(n.label)} · ${n.events} events`}</title>
                </g>
              );
            })}
            {/* collapsed project super-nodes */}
            {model.clusters.filter((c) => c.collapsed).map((c) => {
              const r = clusterR(c.count);
              const col = projectColor(c.project);
              const dim = clusterDim(c.project);
              return (
                <g key={`c-${c.project}`} data-canvas-cluster={c.project} transform={`translate(${c.x},${c.y})`} opacity={dim ? 0.15 : 1} className="cursor-pointer" onClick={() => toggle(c.project)}>
                  <circle r={r + 5} fill={col} fillOpacity={0.12} />
                  <circle r={r} fill={col} fillOpacity={0.85} stroke="#16171f" strokeWidth={2} />
                  <text y={2} textAnchor="middle" fontSize={Math.min(13, r / 1.8)} fontWeight={700} fill="#16171f">{c.count}</text>
                  <text y={r + 13} textAnchor="middle" fontSize={10} fill="#a9b1d6">{c.project.replace(/^-/, "").split(/[-/]/).pop()?.slice(0, 18)}</text>
                </g>
              );
            })}
          </g>
        </svg>
      </div>
    </div>
  );
}
