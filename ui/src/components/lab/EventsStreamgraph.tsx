/** EventsStreamgraph — Lab candidate `events-streamgraph`. Per-agent event
 *  volume over time as a wiggle-offset streamgraph: x = day, band thickness =
 *  that agent's events that day. Shows WHEN each agent was active and how the
 *  fleet's work ebbed and flowed — bursts, handoffs, quiet stretches. Pure
 *  day-bucketing in lib/events-streamgraph; sessions-list only (no records). */

import { useMemo } from "react";
import { scaleLinear } from "d3-scale";
import { stack, stackOffsetWiggle, stackOrderInsideOut, area, curveBasis } from "d3-shape";
import type { StorySession } from "@/lib/story-api";
import { buildStreamgraph, type StreamRow } from "@/lib/events-streamgraph";
import { agentColor } from "@/lib/agent-color";

const W = 860, H = 420, PAD_L = 8, PAD_R = 8, PAD_TB = 24, AXIS_H = 22;

export function EventsStreamgraph({ sessions }: { sessions: readonly StorySession[] }) {
  const g = useMemo(() => buildStreamgraph(sessions, { topAgents: 8 }), [sessions]);

  const { paths, xTicks, plotH } = useMemo(() => {
    if (g.rows.length === 0) return { paths: [], xTicks: [] as { x: number; label: string }[], plotH: 0 };
    const series = stack<StreamRow>().keys(g.agents).offset(stackOffsetWiggle).order(stackOrderInsideOut)(g.rows);

    let lo = Infinity, hi = -Infinity;
    for (const s of series) for (const p of s) { if (p[0]! < lo) lo = p[0]!; if (p[1]! > hi) hi = p[1]!; }

    const plotH = H - AXIS_H;
    const x = scaleLinear().domain([0, Math.max(1, g.rows.length - 1)]).range([PAD_L, W - PAD_R]);
    const y = scaleLinear().domain([lo, hi]).range([plotH - PAD_TB, PAD_TB]);
    const gen = area<[number, number]>()
      .x((_d, i) => x(i))
      .y0((d) => y(d[0]!))
      .y1((d) => y(d[1]!))
      .curve(curveBasis);

    const paths = series.map((s) => ({ agent: s.key, d: gen(s as unknown as [number, number][]) ?? "" }));

    // ~6 date ticks along the day axis
    const n = g.days.length;
    const step = Math.max(1, Math.floor(n / 6));
    const xTicks: { x: number; label: string }[] = [];
    for (let i = 0; i < n; i += step) xTicks.push({ x: x(i), label: g.days[i]!.slice(5) }); // MM-DD
    return { paths, xTicks, plotH };
  }, [g]);

  if (g.rows.length === 0) return <div className="p-6 text-[12px] text-[#565f89]">No dated sessions to stream.</div>;

  return (
    <div className="overflow-auto p-3" style={{ maxHeight: "82vh" }}>
      <div className="mb-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-[#565f89]">
        <span>band = an agent’s events per day · x = time · {g.totalEvents.toLocaleString()} events over {g.days.length} days</span>
        {g.agents.map((a) => (
          <span key={a} className="inline-flex items-center gap-1">
            <span className="inline-block h-2 w-2 rounded-sm" style={{ background: agentColor(a) }} />
            <span className="text-[#a9b1d6]">{a}</span>
          </span>
        ))}
      </div>
      <svg width={W} height={H} className="block">
        {paths.map((p) => (
          <path key={p.agent} d={p.d} fill={agentColor(p.agent)} fillOpacity={0.82} stroke={agentColor(p.agent)} strokeOpacity={0.35} strokeWidth={0.5}>
            <title>{p.agent}</title>
          </path>
        ))}
        {/* day axis */}
        <line x1={PAD_L} x2={W - PAD_R} y1={plotH + 4} y2={plotH + 4} stroke="#2f3348" />
        {xTicks.map((t, i) => (
          <g key={i}>
            <line x1={t.x} x2={t.x} y1={plotH + 4} y2={plotH + 8} stroke="#2f3348" />
            <text x={t.x} y={plotH + 19} textAnchor="middle" fontSize={9} fill="#565f89">{t.label}</text>
          </g>
        ))}
      </svg>
    </div>
  );
}
