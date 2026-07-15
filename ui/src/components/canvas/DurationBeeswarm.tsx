/** DurationBeeswarm — Lab candidate `duration-beeswarm`. One dot per session,
 *  x = wall-clock duration (log scale), packed into a non-overlapping swarm per
 *  agent lane. Surfaces the full duration DISTRIBUTION per agent — modes, gaps,
 *  and long-tail outliers the Scatter can't show. Pure packing in lib/beeswarm;
 *  sessions-only (no records). */

import { useMemo } from "react";
import { scaleLog } from "d3-scale";
import type { StorySession } from "@/lib/story-api";
import { sessionDurationMs } from "@/lib/sessions-overview";
import { beeswarmOffsets } from "@/lib/beeswarm";
import { agentColor } from "@/lib/agent-color";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { formatDuration } from "@/lib/time";

const W = 820, LEFT = 112, R = 2.7, LANE_PAD = 16, AXIS_H = 26;

export function DurationBeeswarm({ sessions, onOpenSession }: {
  sessions: readonly StorySession[];
  /** Dot click → open that session (the canvas side panel). */
  onOpenSession?: (id: string) => void;
}) {
  const { lanes, height, ticks } = useMemo(() => {
    const withDur = sessions
      .map((s) => ({ id: s.session_id, label: s.label || s.session_id.slice(0, 8), agent: s.origin_agent || "unknown", dur: sessionDurationMs(s) }))
      .filter((d) => d.dur > 0);
    const durs = withDur.map((d) => d.dur);
    const min = Math.max(1000, Math.min(...durs, 1000));
    const max = Math.max(...durs, min * 10);
    const x = scaleLog().domain([min, max]).range([LEFT, W - 24]).clamp(true);

    // group by agent, most sessions first
    const byAgent = new Map<string, typeof withDur>();
    for (const d of withDur) (byAgent.get(d.agent) ?? byAgent.set(d.agent, []).get(d.agent)!).push(d);
    const agents = [...byAgent.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]));

    let y = 8;
    const lanes = agents.map(([agent, list]) => {
      const xs = list.map((d) => x(d.dur));
      const offs = beeswarmOffsets(xs, R);
      const maxAbs = Math.max(R, ...offs.map(Math.abs));
      const center = y + maxAbs + R;
      const dots = list.map((d, i) => ({ ...d, cx: xs[i]!, cy: center + offs[i]! }));
      y = center + maxAbs + R + LANE_PAD;
      return { agent, count: list.length, center, dots };
    });
    // log ticks as durations
    const ticks = x.ticks(5).map((t) => ({ x: x(t), label: formatDuration(t) }));
    return { lanes, height: y + AXIS_H, ticks };
  }, [sessions]);

  if (lanes.length === 0) return <div className="p-6 text-[12px] text-[color:var(--text-muted)]">No sessions with a measurable duration.</div>;

  return (
    <div className="overflow-auto p-3" style={{ maxHeight: "80vh" }}>
      <div className="mb-2 text-[11px] text-[color:var(--text-muted)]">one dot = a session · x = duration (log) · lane + color = agent · {lanes.reduce((n, l) => n + l.count, 0)} sessions</div>
      <svg width={W} height={height} className="block">
        {/* x axis */}
        <line x1={LEFT} x2={W - 24} y1={height - AXIS_H} y2={height - AXIS_H} stroke="#2f3348" />
        {ticks.map((t, i) => (
          <g key={i}>
            <line x1={t.x} x2={t.x} y1={height - AXIS_H} y2={height - AXIS_H + 4} stroke="#2f3348" />
            <text x={t.x} y={height - AXIS_H + 16} textAnchor="middle" fontSize={9} fill="#565f89">{t.label}</text>
          </g>
        ))}
        {lanes.map((lane) => (
          <g key={lane.agent}>
            <rect x={LEFT - 12} y={lane.center - 4} width={7} height={7} rx={1.5} fill={agentColor(lane.agent)} />
            <text x={LEFT - 16} y={lane.center + 4} textAnchor="end" fontSize={10} fill="#a9b1d6">{lane.agent}</text>
            <text x={LEFT - 16} y={lane.center + 15} textAnchor="end" fontSize={8} fill="#565f89">{lane.count}</text>
            {lane.dots.map((d) => (
              <circle
                key={d.id}
                data-session-id={d.id}
                cx={d.cx}
                cy={d.cy}
                r={R}
                fill={agentColor(lane.agent)}
                fillOpacity={0.66}
                className={onOpenSession ? "cursor-pointer hover:fill-opacity-100" : undefined}
                onClick={onOpenSession ? () => onOpenSession(d.id) : undefined}
              >
                <title>{`${cleanHarnessPreview(d.label)} · ${formatDuration(d.dur)}`}</title>
              </circle>
            ))}
          </g>
        ))}
      </svg>
    </div>
  );
}
