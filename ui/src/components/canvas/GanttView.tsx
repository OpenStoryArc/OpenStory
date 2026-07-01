/** GanttView — the Fleet Gantt: concurrent sessions as lane-packed bars on a
 *  time axis, banded by a group dimension, with a brushable overview strip that
 *  controls the visible window. Answers "what's latest" (defaults to the recent
 *  tail) and shows concurrency. Bar length = duration, color = agent, ongoing
 *  sessions pulse. Click a bar → session detail. Pure packing in lib/sessions-gantt. */

import { useEffect, useMemo, useRef, useState } from "react";
import { select } from "d3-selection";
import { brushX } from "d3-brush";
import { scaleTime } from "d3-scale";
import { timeFormat } from "d3-time-format";
import type { StorySession } from "@/lib/story-api";
import type { GroupDim } from "@/lib/sessions-canvas";
import { buildGantt } from "@/lib/sessions-gantt";
import { agentColor } from "@/lib/agent-color";
import { cleanHarnessPreview } from "@/lib/harness-message";

interface Props {
  sessions: readonly StorySession[];
  groupBy: GroupDim;
  width: number;
  height: number;
  nowMs: number;
  onOpenSession: (id: string) => void;
}

const GUTTER = 96;
const LANE_H = 14;
const OV_H = 46;
const AXIS_H = 18;
const fmt = timeFormat("%b %-d, %H:%M");

export function GanttView({ sessions, groupBy, width, height, nowMs, onOpenSession }: Props) {
  const model = useMemo(() => buildGantt(sessions, groupBy, nowMs), [sessions, groupBy, nowMs]);
  const [domain0, domain1] = model.domain;
  // default visible window = the most recent slice (answers "what's latest")
  const defaultSpan = Math.min((domain1 - domain0) || 1, 3 * 24 * 3600_000);
  const [view, setView] = useState<[number, number]>([Math.max(domain0, domain1 - defaultSpan), domain1]);
  useEffect(() => { setView([Math.max(domain0, domain1 - defaultSpan), domain1]); }, [domain0, domain1]); // eslint-disable-line react-hooks/exhaustive-deps

  const plotW = Math.max(width - GUTTER - 10, 50);
  const x = useMemo(() => scaleTime().domain([new Date(view[0]), new Date(view[1])]).range([GUTTER, GUTTER + plotW]), [view, plotW]);
  const xOv = useMemo(() => scaleTime().domain([new Date(domain0), new Date(domain1)]).range([GUTTER, GUTTER + plotW]), [domain0, domain1, plotW]);

  const topH = height - OV_H - AXIS_H - 4;
  const brushRef = useRef<SVGGElement>(null);

  // overview brush controls the visible window
  useEffect(() => {
    const g = brushRef.current;
    if (!g) return;
    const sel = select(g);
    const b = brushX<unknown>().extent([[GUTTER, 0], [GUTTER + plotW, OV_H - 14]])
      .on("brush end", (e) => {
        if (!e.selection) return;
        const [a, c] = e.selection as [number, number];
        setView([xOv.invert(a).getTime(), xOv.invert(c).getTime()]);
      });
    sel.call(b);
    sel.call(b.move, [xOv(new Date(view[0])), xOv(new Date(view[1]))]);
    return () => { sel.on(".brush", null); };
  }, [xOv, plotW]); // eslint-disable-line react-hooks/exhaustive-deps

  const ticks = x.ticks(Math.max(3, Math.floor(plotW / 130)));
  const visibleBars = model.bars.filter((bar) => bar.endMs >= view[0] && bar.startMs <= view[1]);

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-[#16171f]">
      {/* scrollable lane plot */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <svg width={width} height={Math.max(topH, model.laneCount * LANE_H + 8)} className="block">
          {/* band labels + separators */}
          {model.bands.map((band) => (
            <g key={band.name}>
              <line x1={0} x2={width} y1={band.laneStart * LANE_H} y2={band.laneStart * LANE_H} stroke="#2f3348" strokeWidth={0.5} />
              <text x={6} y={band.laneStart * LANE_H + 11} fontSize={10} fill="#a9b1d6">{cleanHarnessPreview(band.name).split(/[/]/).pop()?.slice(0, 12)}</text>
            </g>
          ))}
          {/* bars */}
          {visibleBars.map((bar) => {
            const bx = Math.max(x(new Date(bar.startMs)), GUTTER);
            const bw = Math.max(x(new Date(Math.min(bar.endMs, view[1]))) - bx, 2);
            const col = agentColor(bar.agent);
            return (
              <rect
                key={bar.id} data-gantt-bar={bar.id}
                x={bx} y={bar.lane * LANE_H + 1.5} width={bw} height={LANE_H - 3} rx={2}
                fill={col} fillOpacity={bar.ongoing ? 0.95 : 0.72}
                className={bar.ongoing ? "cursor-pointer pulse-live" : "cursor-pointer"}
                stroke={bar.ongoing ? "#c0caf5" : "none"} strokeWidth={bar.ongoing ? 1 : 0}
                onClick={() => onOpenSession(bar.id)}
              >
                <title>{`${cleanHarnessPreview(bar.label)} · ${bar.agent}${bar.ongoing ? " · ongoing" : ""}`}</title>
              </rect>
            );
          })}
        </svg>
      </div>

      {/* time axis */}
      <svg width={width} height={AXIS_H} className="block shrink-0">
        <line x1={GUTTER} x2={GUTTER + plotW} y1={0} y2={0} stroke="#2f3348" />
        {ticks.map((tk, i) => (
          <text key={i} x={x(tk)} y={13} textAnchor="middle" fontSize={9} fill="#565f89">{fmt(tk)}</text>
        ))}
      </svg>

      {/* overview + brush */}
      <svg width={width} height={OV_H} className="block shrink-0 border-t border-[#2f3348]">
        <text x={6} y={12} fontSize={9} fill="#565f89">overview · drag to window</text>
        {model.bars.map((bar) => (
          <rect key={bar.id} x={xOv(new Date(bar.startMs))} y={16 + (bar.lane % 6) * 3} width={Math.max(xOv(new Date(bar.endMs)) - xOv(new Date(bar.startMs)), 1)} height={2} fill={agentColor(bar.agent)} fillOpacity={0.6} />
        ))}
        <g ref={brushRef} />
      </svg>
    </div>
  );
}
