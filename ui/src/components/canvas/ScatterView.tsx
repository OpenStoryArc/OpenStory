/** ScatterView — the efficiency scatter: events × output-tokens in log-log
 *  space with an OLS fit line + ±1σ band, so sessions off the regression pop as
 *  outliers. Size = duration, color = agent. Zero-output sessions
 *  (uninstrumented agents) sit in a labeled gutter rather than being dropped.
 *  Click a point → open its session. Pure model + fit in lib/sessions-scatter. */

import { useMemo, useState } from "react";
import { scaleLog, scaleSqrt } from "d3-scale";
import type { StorySession } from "@/lib/story-api";
import { buildScatter, type ScatterPoint } from "@/lib/sessions-scatter";
import { agentColor } from "@/lib/agent-color";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { formatDuration } from "@/lib/time";

interface Props {
  sessions: readonly StorySession[];
  width: number;
  height: number;
  onOpenSession: (id: string) => void;
}

const M = { top: 24, right: 20, bottom: 40, left: 56 };
const GUTTER_W = 34; // "0 tokens" column

function quantile(sorted: number[], p: number): number {
  if (!sorted.length) return 0;
  return sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))]!;
}

export function ScatterView({ sessions, width, height, onOpenSession }: Props) {
  const model = useMemo(() => buildScatter(sessions), [sessions]);
  const [hover, setHover] = useState<{ p: ScatterPoint; x: number; y: number } | null>(null);

  const plotLeft = M.left + GUTTER_W;
  const plotRight = width - M.right;
  const plotTop = M.top;
  const plotBottom = height - M.bottom;

  const maxEv = Math.max(10, ...model.points.map((p) => p.events));
  const maxTok = Math.max(10, ...model.points.map((p) => p.tokens));
  const x = useMemo(() => scaleLog().domain([1, maxEv]).range([plotLeft, plotRight]).clamp(true), [maxEv, plotLeft, plotRight]);
  const y = useMemo(() => scaleLog().domain([1, maxTok]).range([plotBottom, plotTop]).clamp(true), [maxTok, plotBottom, plotTop]);

  // winsorize duration at p99 for sizing
  const rSize = useMemo(() => {
    const ds = model.points.map((p) => p.durationMs).sort((a, b) => a - b);
    const cap = quantile(ds, 0.99) || 1;
    return scaleSqrt().domain([0, cap]).range([2.5, 9]).clamp(true);
  }, [model]);

  const fit = model.fit;
  const fitPath = useMemo(() => {
    if (!fit) return null;
    const yAt = (ev: number) => Math.pow(10, fit.slope * Math.log10(ev) + fit.intercept);
    const line = (mul: number) => [1, maxEv].map((ev) => `${x(ev)},${y(Math.max(1, yAt(ev) * mul))}`).join(" L");
    return { mid: `M${line(1)}`, hi: `M${line(Math.pow(10, fit.sigma))}`, lo: `M${line(Math.pow(10, -fit.sigma))}` };
  }, [fit, x, y, maxEv]);

  const xTicks = x.ticks(4);
  const yTicks = y.ticks(4);
  const kfmt = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(0)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(0)}k` : String(n));

  return (
    <div className="relative min-h-0 flex-1 bg-[#16171f]">
      <svg width={width} height={height} className="block">
        {/* axes */}
        <line x1={plotLeft} x2={plotRight} y1={plotBottom} y2={plotBottom} stroke="#2f3348" />
        <line x1={plotLeft} x2={plotLeft} y1={plotTop} y2={plotBottom} stroke="#2f3348" />
        {xTicks.map((t, i) => (
          <g key={`x${i}`}>
            <line x1={x(t)} x2={x(t)} y1={plotBottom} y2={plotBottom + 4} stroke="#2f3348" />
            <text x={x(t)} y={plotBottom + 15} textAnchor="middle" fontSize={9} fill="#565f89">{kfmt(t)}</text>
          </g>
        ))}
        {yTicks.map((t, i) => (
          <g key={`y${i}`}>
            <line x1={plotLeft - 4} x2={plotLeft} y1={y(t)} y2={y(t)} stroke="#2f3348" />
            <text x={plotLeft - 7} y={y(t) + 3} textAnchor="end" fontSize={9} fill="#565f89">{kfmt(t)}</text>
          </g>
        ))}
        <text x={(plotLeft + plotRight) / 2} y={height - 6} textAnchor="middle" fontSize={10} fill="#a9b1d6">events (log)</text>
        <text transform={`translate(12,${(plotTop + plotBottom) / 2}) rotate(-90)`} textAnchor="middle" fontSize={10} fill="#a9b1d6">output tokens (log)</text>

        {/* zero / uninstrumented gutter */}
        <rect x={M.left} y={plotTop} width={GUTTER_W} height={plotBottom - plotTop} fill="#565f89" fillOpacity={0.05} />
        <text x={M.left + GUTTER_W / 2} y={plotBottom + 15} textAnchor="middle" fontSize={8} fill="#565f89">0 tok</text>

        {/* fit line + ±1σ band */}
        {fitPath && (
          <g>
            <path d={fitPath.hi} fill="none" stroke="#565f89" strokeOpacity={0.35} strokeDasharray="3 3" />
            <path d={fitPath.lo} fill="none" stroke="#565f89" strokeOpacity={0.35} strokeDasharray="3 3" />
            <path d={fitPath.mid} fill="none" stroke="#c0caf5" strokeOpacity={0.5} strokeWidth={1.4} />
          </g>
        )}

        {/* points */}
        {model.points.map((p) => {
          const px = p.zero ? M.left + GUTTER_W / 2 : x(Math.max(1, p.events));
          const py = p.zero ? plotTop + ((hashJitter(p.id) * (plotBottom - plotTop))) : y(Math.max(1, p.tokens));
          return (
            <circle
              key={p.id} data-scatter-point={p.id}
              cx={px} cy={py} r={rSize(p.durationMs)} fill={agentColor(p.agent)} fillOpacity={0.62}
              className="cursor-pointer"
              onMouseEnter={() => setHover({ p, x: px, y: py })}
              onMouseLeave={() => setHover((h) => (h?.p.id === p.id ? null : h))}
              onClick={() => onOpenSession(p.id)}
            />
          );
        })}
      </svg>

      {hover && (
        <div className="pointer-events-none absolute z-10 rounded border border-[#2f3348] bg-[#1a1b26] px-2 py-1 text-[10px] text-[#c0caf5] shadow-lg" style={{ left: Math.min(hover.x + 10, width - 160), top: hover.y - 8 }}>
          <div className="max-w-[150px] truncate">{cleanHarnessPreview(hover.p.label)}</div>
          <div className="text-[#565f89]">{hover.p.events} ev · {kfmt(hover.p.tokens)} out · {formatDuration(hover.p.durationMs)} · <span style={{ color: agentColor(hover.p.agent) }}>{hover.p.agent}</span></div>
        </div>
      )}
    </div>
  );
}

/** deterministic [0,1) jitter from an id, to spread zero-token points vertically. */
function hashJitter(id: string): number {
  let h = 0;
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) | 0;
  return ((h >>> 0) % 1000) / 1000;
}
