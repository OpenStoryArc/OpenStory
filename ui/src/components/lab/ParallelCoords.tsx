/** ParallelCoords — Lab candidate `session-parallel-coords`. Each session is a
 *  polyline across four axes (events / input tokens / output tokens / duration),
 *  each normalized independently to [0,1]. Lines colored by agent. Multivariate
 *  shape at a glance: a session high on events but low on tokens crosses the
 *  bundle; agent clusters read as coherent ribbons. Pure normalization in
 *  lib/parallel-coords; sessions-list only (no records). Brushing: later. */

import { useMemo, useState } from "react";
import { scalePoint, scaleLinear } from "d3-scale";
import { line as d3line, curveMonotoneX } from "d3-shape";
import type { StorySession } from "@/lib/story-api";
import { buildParallelCoords } from "@/lib/parallel-coords";
import { agentColor } from "@/lib/agent-color";
import { formatDuration } from "@/lib/time";

const W = 860, H = 460, PAD_X = 90, PAD_TOP = 40, PAD_BOT = 40;

/** Per-axis label formatter for the domain min/max ticks. */
function fmt(key: string, v: number): string {
  if (key === "duration") return formatDuration(v);
  if (key === "in" || key === "out") return v >= 1000 ? `${(v / 1000).toFixed(0)}k` : `${v}`;
  return v.toLocaleString();
}

export function ParallelCoords({ sessions }: { sessions: readonly StorySession[] }) {
  const pc = useMemo(() => buildParallelCoords(sessions), [sessions]);
  const [hoverAgent, setHoverAgent] = useState<string | null>(null);

  const { x, paths, agents } = useMemo(() => {
    const x = scalePoint<string>().domain(pc.axes.map((a) => a.key)).range([PAD_X, W - PAD_X]);
    const y = scaleLinear().domain([0, 1]).range([H - PAD_BOT, PAD_TOP]);
    const gen = d3line<{ ax: string; v: number }>().x((d) => x(d.ax)!).y((d) => y(d.v)).curve(curveMonotoneX);
    const paths = pc.lines.map((l) => ({
      id: l.session_id, agent: l.agent, label: l.label,
      d: gen(pc.axes.map((a, j) => ({ ax: a.key, v: l.coords[j]! }))) ?? "",
    }));
    const agents = [...new Set(pc.lines.map((l) => l.agent))].sort();
    return { x, paths, agents };
  }, [pc]);

  if (pc.lines.length === 0) return <div className="p-6 text-[12px] text-[#565f89]">No sessions to plot.</div>;

  return (
    <div className="overflow-auto p-3" style={{ maxHeight: "82vh" }}>
      <div className="mb-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-[#565f89]">
        <span>one polyline = a session · each axis normalized to its own min…max · {pc.lines.length} sessions</span>
        {agents.map((a) => (
          <span key={a} className="inline-flex cursor-pointer items-center gap-1"
            onMouseEnter={() => setHoverAgent(a)} onMouseLeave={() => setHoverAgent(null)}>
            <span className="inline-block h-2 w-2 rounded-sm" style={{ background: agentColor(a) }} />
            <span className={hoverAgent === a ? "text-[#c0caf5]" : "text-[#a9b1d6]"}>{a}</span>
          </span>
        ))}
      </div>
      <svg width={W} height={H} className="block">
        {/* lines (dim when another agent is hovered) */}
        {paths.map((p) => {
          const active = !hoverAgent || hoverAgent === p.agent;
          return (
            <path key={p.id} d={p.d} fill="none" stroke={agentColor(p.agent)}
              strokeWidth={hoverAgent === p.agent ? 1.1 : 0.7}
              strokeOpacity={active ? (hoverAgent ? 0.5 : 0.28) : 0.04}>
              <title>{`${p.label} · ${p.agent}`}</title>
            </path>
          );
        })}
        {/* axes */}
        {pc.axes.map((a, j) => {
          const ax = x(a.key)!;
          const [min, max] = pc.domains[j]!;
          return (
            <g key={a.key}>
              <line x1={ax} x2={ax} y1={PAD_TOP} y2={H - PAD_BOT} stroke="#3b4261" strokeWidth={1} />
              <text x={ax} y={PAD_TOP - 12} textAnchor="middle" fontSize={11} fill="#c0caf5" fontWeight={600}>{a.label}</text>
              <text x={ax} y={PAD_TOP - 1} textAnchor="middle" fontSize={9} fill="#565f89">{fmt(a.key, max)}</text>
              <text x={ax} y={H - PAD_BOT + 13} textAnchor="middle" fontSize={9} fill="#565f89">{fmt(a.key, min)}</text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}
