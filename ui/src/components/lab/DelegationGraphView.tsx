/** DelegationGraphView — Lab candidate `delegation-graph`, the flagship. The
 *  live spawn topology no other tool shows: parent sessions → the subagents they
 *  launched. Samples the biggest non-subagent sessions' records, resolves
 *  parent→child via the echoed agentId (pure buildDelegationGraph), and lays it
 *  out with d3-force. Node size = events, color = status. */

import { useEffect, useMemo, useState } from "react";
import { forceSimulation, forceManyBody, forceLink, forceCenter, forceCollide } from "d3-force";
import type { StorySession } from "@/lib/story-api";
import { buildDelegationGraph, type DelRecord } from "@/lib/delegation-graph";
import { fitTransform } from "@/lib/canvas-fit";

const W = 780, H = 540, SAMPLE = 10;
type SimNode = { id: string; label: string; events: number; status: string; isSub: boolean; x?: number; y?: number };

const statusColor = (s: string): string =>
  s === "ongoing" ? "#9ece6a" : s === "errored" || s === "error" ? "#f7768e" : s === "completed" ? "#7aa2f7" : "#565f89";
const radius = (ev: number): number => Math.max(4, Math.min(16, 3 + Math.sqrt(ev) / 3));

export function DelegationGraphView({ sessions }: { sessions: readonly StorySession[] }) {
  const [recordsBySession, setRBS] = useState<Record<string, DelRecord[]>>({});
  const [loading, setLoading] = useState(true);

  // biggest top-level sessions are the likely delegators — sample their records.
  const parents = useMemo(
    () => sessions.filter((s) => !s.session_id.startsWith("agent-")).slice().sort((a, b) => (b.event_count ?? 0) - (a.event_count ?? 0)).slice(0, SAMPLE),
    [sessions],
  );

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    const fetchRecs = async (id: string): Promise<DelRecord[]> => {
      const ctrl = new AbortController();
      const timer = setTimeout(() => ctrl.abort(), 25000); // delegators are big (multi-MB records)
      try {
        const r = await fetch(`/api/sessions/${id}/records`, { signal: ctrl.signal });
        const j = await r.json();
        return Array.isArray(j) ? (j as DelRecord[]) : [];
      } catch { return []; } finally { clearTimeout(timer); }
    };
    Promise.all(parents.map(async (s) => [s.session_id, await fetchRecs(s.session_id)] as const)).then((pairs) => {
      if (cancelled) return;
      const rbs: Record<string, DelRecord[]> = {};
      for (const [id, recs] of pairs) rbs[id] = recs;
      setRBS(rbs);
      setLoading(false);
    });
    return () => { cancelled = true; };
  }, [parents]);

  const graph = useMemo(() => buildDelegationGraph(sessions, recordsBySession), [sessions, recordsBySession]);

  const laid = useMemo(() => {
    const nodes: SimNode[] = graph.nodes.map((n) => ({ ...n }));
    const links = graph.links.map((l) => ({ source: l.source, target: l.target }));
    if (nodes.length === 0) return { nodes: [] as SimNode[], links: [] as { source: SimNode; target: SimNode }[], fit: { k: 1, x: 0, y: 0 } };
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const sim = forceSimulation(nodes as any)
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .force("charge", forceManyBody().strength(-90))
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .force("link", forceLink(links as any).id((d: any) => d.id).distance(38).strength(0.6))
      .force("center", forceCenter(W / 2, H / 2))
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .force("collide", forceCollide((d: any) => radius(d.events) + 2))
      .stop();
    for (let i = 0; i < 320; i++) sim.tick();
    // frame the settled layout into the viewport (force drifts off-center).
    const xs = nodes.map((n) => n.x ?? 0), ys = nodes.map((n) => n.y ?? 0);
    const fit = fitTransform(
      { minX: Math.min(...xs), maxX: Math.max(...xs), minY: Math.min(...ys), maxY: Math.max(...ys) },
      { w: W, h: H }, { pad: 40, maxScale: 1.4 },
    );
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return { nodes, links: links as any[], fit };
  }, [graph]);

  if (loading) return <div className="p-6 text-[12px] text-[#565f89]">Sampling parent records + resolving spawns…</div>;
  if (laid.nodes.length === 0) return <div className="p-6 text-[12px] text-[#565f89]">No parent→subagent links resolved in the sampled parents.</div>;

  return (
    <div className="p-3">
      <div className="mb-2 text-[11px] text-[#565f89]">
        parent → subagent spawns · node size = events · color = status · {graph.resolvedSubs} of {graph.totalSubs} subagents resolved (top-{parents.length} parents sampled)
      </div>
      <svg width={W} height={H} className="block rounded bg-[#141520]">
        <g transform={`translate(${laid.fit.x},${laid.fit.y}) scale(${laid.fit.k})`}>
          {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
          {laid.links.map((l: any, i: number) => (
            <line key={i} x1={l.source.x} y1={l.source.y} x2={l.target.x} y2={l.target.y} stroke="#3b4261" strokeWidth={1} strokeOpacity={0.6} />
          ))}
          {laid.nodes.map((n) => (
            <circle
              key={n.id} data-node={n.id} cx={n.x} cy={n.y} r={radius(n.events)}
              fill={statusColor(n.status)} fillOpacity={n.isSub ? 0.72 : 0.95}
              stroke={n.isSub ? "none" : "#c0caf5"} strokeWidth={n.isSub ? 0 : 1.5}
            >
              <title>{`${n.label} · ${n.events} ev · ${n.status}${n.isSub ? " · subagent" : " · parent"}`}</title>
            </circle>
          ))}
        </g>
      </svg>
    </div>
  );
}
