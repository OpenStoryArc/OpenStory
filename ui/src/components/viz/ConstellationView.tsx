/** ConstellationView — an interactive pan-zoom canvas of a session and the
 *  subagents it spawned (its delegation graph). Scroll to zoom, drag to pan,
 *  click a node to open that session. The "map of your agent work" no other
 *  tool shows. D3 supplies zoom behavior + the size scale; the graph model is
 *  the pure buildConstellation. */

import { useEffect, useMemo, useRef, useState } from "react";
import { select } from "d3-selection";
import { zoom as d3zoom, zoomIdentity } from "d3-zoom";
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

const PAD = 60;

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
    return scaleSqrt().domain([0, maxEv]).range([10, 34]).clamp(true);
  }, [graph]);

  // pan/zoom
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const sel = select(svg);
    const z = d3zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.35, 4])
      .on("zoom", (e) => setT({ k: e.transform.k, x: e.transform.x, y: e.transform.y }));
    sel.call(z);
    sel.call(z.transform, zoomIdentity);
    return () => { sel.on(".zoom", null); };
  }, [rootId]);

  const px = (n: ConstellationNode) => ({ x: PAD + n.x * (width - 2 * PAD), y: PAD + n.y * (height - 2 * PAD) });

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
      <svg ref={svgRef} width={width} height={height} className="block cursor-grab active:cursor-grabbing" style={{ touchAction: "none" }}>
        <g transform={`translate(${t.x},${t.y}) scale(${t.k})`}>
          {/* edges */}
          {graph.edges.map((e) => {
            const a = nodeById.get(e.from)!;
            const b = nodeById.get(e.to)!;
            const pa = px(a); const pb = px(b);
            return <line key={`${e.from}-${e.to}`} x1={pa.x} y1={pa.y} x2={pb.x} y2={pb.y} stroke="#3b4261" strokeWidth={1.5} strokeOpacity={0.7} />;
          })}
          {/* nodes */}
          {graph.nodes.map((n) => {
            const p = px(n);
            const r = n.kind === "root" ? rScale(n.events) + 6 : rScale(n.events);
            const color = statusColor(n);
            const clickable = Boolean(onOpen && n.linked);
            return (
              <g key={n.id} data-con-node={n.id} transform={`translate(${p.x},${p.y})`} className={clickable ? "cursor-pointer" : undefined} onClick={clickable ? () => onOpen!(n.id) : undefined}>
                <circle r={r + 4} fill={color} fillOpacity={0.12} />
                <circle r={r} fill={color} fillOpacity={n.kind === "root" ? 0.9 : 0.7} stroke={n.kind === "root" ? "#c0caf5" : color} strokeWidth={n.kind === "root" ? 2 : 1} />
                <text y={r + 12} textAnchor="middle" fontSize={n.kind === "root" ? 11 : 10} fill={n.kind === "root" ? "#c0caf5" : "#a9b1d6"}>
                  {cleanHarnessPreview(n.label).slice(0, 28)}
                </text>
                {n.kind === "subagent" && n.subagentType && (
                  <text y={r + 24} textAnchor="middle" fontSize={8} fill="#565f89">{n.subagentType}</text>
                )}
                {n.events > 0 && (
                  <text y={3} textAnchor="middle" fontSize={9} fill="#1a1b26" fontWeight={600}>{n.events}</text>
                )}
              </g>
            );
          })}
        </g>
      </svg>
    </div>
  );
}
