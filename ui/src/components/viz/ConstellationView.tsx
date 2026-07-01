/** ConstellationView — an interactive pan-zoom canvas of a session and the
 *  subagents it spawned (its delegation graph). Scroll to zoom, drag to pan,
 *  click a node to open that session. The "map of your agent work" no other
 *  tool shows. D3 supplies zoom behavior + the size scale; the graph model is
 *  the pure buildConstellation. */

import { useEffect, useMemo, useRef, useState } from "react";
import { select } from "d3-selection";
import { zoom as d3zoom, zoomIdentity, type ZoomBehavior } from "d3-zoom";
import { scaleSqrt } from "d3-scale";
import type { WireRecord } from "@/types/wire-record";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { buildConstellation, type ConstellationNode } from "@/lib/constellation";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { sessionColor } from "@/lib/session-colors";
import { cn } from "@/lib/cn";

interface Props {
  rootId: string;
  height?: number;
  onOpen?: (sessionId: string) => void;
  className?: string;
}

const PAD = 70;

function statusColor(n: ConstellationNode): string {
  if (n.isError || n.status === "error") return "#f7768e";
  if (n.status === "ongoing") return "#9ece6a";
  if (!n.linked) return "#565f89";
  return sessionColor(n.id);
}

export function ConstellationView({ rootId, height = 480, onOpen, className }: Props) {
  const { sessions } = useSessionsList();
  const [records, setRecords] = useState<WireRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const wrapRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const zoomRef = useRef<ZoomBehavior<SVGSVGElement, unknown> | null>(null);
  const [width, setWidth] = useState(800);
  const [t, setT] = useState({ k: 1, x: 0, y: 0 });

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    fetch(`/api/sessions/${rootId}/records`)
      .then((r) => r.json())
      .then((data: WireRecord[]) => { if (!cancelled) { setRecords(Array.isArray(data) ? data : []); setLoading(false); } })
      .catch(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [rootId]);

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const measure = () => setWidth(el.clientWidth || 800);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const sessionsById = useMemo(() => new Map(sessions.map((s) => [s.session_id, s])), [sessions]);
  const graph = useMemo(() => buildConstellation(rootId, records, sessionsById), [rootId, records, sessionsById]);

  const rScale = useMemo(() => {
    const maxEv = Math.max(1, ...graph.nodes.map((n) => n.events));
    return scaleSqrt().domain([0, maxEv]).range([12, 38]).clamp(true);
  }, [graph]);

  const px = useMemo(
    () => (n: ConstellationNode) => ({ x: PAD + n.x * (width - 2 * PAD), y: PAD + n.y * (height - 2 * PAD) }),
    [width, height],
  );

  // zoom/pan behavior (created once)
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const sel = select(svg);
    const z = d3zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.2, 4])
      .on("zoom", (e) => setT({ k: e.transform.k, x: e.transform.x, y: e.transform.y }));
    zoomRef.current = z;
    sel.call(z);
    return () => { sel.on(".zoom", null); };
  }, []);

  // auto-fit: frame the whole graph with padding whenever it (or the size) changes
  useEffect(() => {
    const svg = svgRef.current;
    const z = zoomRef.current;
    if (!svg || !z || graph.nodes.length === 0) return;
    const pts = graph.nodes.map(px);
    const rMax = Math.max(...graph.nodes.map((n) => rScale(n.events))) + 30;
    const minX = Math.min(...pts.map((p) => p.x)) - rMax;
    const maxX = Math.max(...pts.map((p) => p.x)) + rMax;
    const minY = Math.min(...pts.map((p) => p.y)) - rMax;
    const maxY = Math.max(...pts.map((p) => p.y)) + rMax;
    const bw = Math.max(maxX - minX, 1);
    const bh = Math.max(maxY - minY, 1);
    const scale = Math.min(width / bw, height / bh, 1.6);
    const tx = width / 2 - scale * (minX + maxX) / 2;
    const ty = height / 2 - scale * (minY + maxY) / 2;
    select(svg).call(z.transform, zoomIdentity.translate(tx, ty).scale(scale));
  }, [graph, width, height, px, rScale]);

  if (loading) return <div className={cn("px-3 py-6 text-[11px] text-[#565f89]", className)}>Loading graph…</div>;
  if (graph.edges.length === 0) {
    return <div className={cn("px-3 py-8 text-center text-[12px] text-[#565f89]", className)} data-testid="constellation-empty">This session spawned no subagents.</div>;
  }

  const nodeById = new Map(graph.nodes.map((n) => [n.id, n]));

  return (
    <div ref={wrapRef} className={cn("relative w-full", className)} data-testid="constellation">
      <div className="pointer-events-none absolute left-2 top-2 z-10 text-[10px] text-[#565f89]">
        {graph.nodes.length - 1} subagents · scroll to zoom · drag to pan
      </div>
      <svg ref={svgRef} width={width} height={height} className="block cursor-grab bg-[#16171f] active:cursor-grabbing" style={{ touchAction: "none" }}>
        <defs>
          {/* Figma-style dot grid backdrop */}
          <pattern id="con-dots" width="22" height="22" patternUnits="userSpaceOnUse">
            <circle cx="1" cy="1" r="1" fill="#2f3348" fillOpacity="0.5" />
          </pattern>
          {/* soft glow for nodes */}
          <filter id="con-glow" x="-60%" y="-60%" width="220%" height="220%">
            <feGaussianBlur stdDeviation="4" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>
        <rect width={width} height={height} fill="url(#con-dots)" />
        <g transform={`translate(${t.x},${t.y}) scale(${t.k})`}>
          {/* curved edges */}
          {graph.edges.map((e) => {
            const a = nodeById.get(e.from)!;
            const b = nodeById.get(e.to)!;
            const pa = px(a); const pb = px(b);
            const dx = pb.x - pa.x, dy = pb.y - pa.y;
            const cx = (pa.x + pb.x) / 2 - dy * 0.14;
            const cy = (pa.y + pb.y) / 2 + dx * 0.14;
            const color = statusColor(b);
            return (
              <path key={`${e.from}-${e.to}`} d={`M${pa.x},${pa.y} Q${cx},${cy} ${pb.x},${pb.y}`} fill="none" stroke={color} strokeOpacity={0.4} strokeWidth={1.5} />
            );
          })}
          {/* nodes */}
          {graph.nodes.map((n) => {
            const p = px(n);
            const r = n.kind === "root" ? rScale(n.events) + 6 : rScale(n.events);
            const color = statusColor(n);
            const clickable = Boolean(onOpen && n.linked);
            return (
              <g key={n.id} data-con-node={n.id} transform={`translate(${p.x},${p.y})`} className={clickable ? "cursor-pointer" : undefined} onClick={clickable ? () => onOpen!(n.id) : undefined}>
                <circle r={r + 8} fill={color} fillOpacity={0.12} />
                <circle r={r} fill={color} fillOpacity={n.kind === "root" ? 0.92 : 0.72} stroke={n.kind === "root" ? "#c0caf5" : color} strokeWidth={n.kind === "root" ? 2 : 1} filter="url(#con-glow)" />
                <text y={r + 13} textAnchor="middle" fontSize={n.kind === "root" ? 11 : 10} fill={n.kind === "root" ? "#c0caf5" : "#a9b1d6"}>
                  {cleanHarnessPreview(n.label).slice(0, 28)}
                </text>
                {n.kind === "subagent" && n.subagentType && (
                  <text y={r + 25} textAnchor="middle" fontSize={8} fill="#565f89">{n.subagentType}</text>
                )}
                {n.events > 0 && (
                  <text y={3.5} textAnchor="middle" fontSize={9} fill="#1a1b26" fontWeight={700}>{n.events}</text>
                )}
              </g>
            );
          })}
        </g>
      </svg>
    </div>
  );
}
