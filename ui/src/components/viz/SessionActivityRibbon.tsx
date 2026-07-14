/** SessionActivityRibbon — a compact D3 timeline of a session's activity.
 *
 *  Renders the pure `buildTimelineModel` output: one swimlane per event kind,
 *  marks positioned in time, a cumulative token-burn area underneath, and a
 *  time axis. Gives at-a-glance visibility into the *shape* of a session —
 *  where the reasoning clustered, where tools fired, where errors landed,
 *  where the token burn spiked — without scrolling the whole transcript.
 *
 *  D3 is used for the math (scaleTime / scaleSqrt / d3-shape area / timeFormat);
 *  marks render as JSX so the component stays React-idiomatic and testable.
 */

import { useMemo, useRef, useState, useLayoutEffect } from "react";
import { scaleTime, scaleLinear, scaleSqrt } from "d3-scale";
import { area, curveMonotoneX } from "d3-shape";
import { timeFormat } from "d3-time-format";
import type { WireRecord } from "@/types/wire-record";
import {
  buildTimelineModel,
  type LaneKey,
  type RibbonEvent,
} from "@/lib/session-timeline";
import { cn } from "@/lib/cn";
import { usePersistedFlag } from "@/hooks/use-persisted-flag";

interface Props {
  records: readonly WireRecord[];
  /** Fixed width (px). When omitted the ribbon measures its container. */
  width?: number;
  className?: string;
  selectedEventId?: string | null;
  onSelectEvent?: (id: string) => void;
}

const LANE_LABEL: Record<LaneKey, string> = {
  user: "user",
  reasoning: "reasoning",
  assistant: "assistant",
  tool: "tool",
  system: "system",
};

const LANE_ROW_FULL = 22; // px per swimlane
const LANE_ROW_COMPACT = 13;
const TOKEN_BAND_FULL = 34; // px for the token-burn area
const TOKEN_BAND_COMPACT = 20;
const AXIS_H = 18;
const PAD_L = 76; // room for lane labels
const PAD_R = 12;
const PAD_T = 8;

const fmtAxis = timeFormat("%H:%M:%S");
const fmtFull = timeFormat("%H:%M:%S");

function humanDuration(ms: number): string {
  if (ms <= 0) return "0s";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

function useMeasuredWidth(explicit: number | undefined): [React.RefObject<HTMLDivElement | null>, number] {
  const ref = useRef<HTMLDivElement | null>(null);
  const [w, setW] = useState(explicit ?? 800);
  useLayoutEffect(() => {
    if (explicit) {
      setW(explicit);
      return;
    }
    const el = ref.current;
    if (!el) return;
    const measure = () => setW(el.clientWidth || 800);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [explicit]);
  return [ref, w];
}

export function SessionActivityRibbon({
  records,
  width,
  className,
  selectedEventId,
  onSelectEvent,
}: Props) {
  const model = useMemo(() => buildTimelineModel(records), [records]);
  const [containerRef, measuredW] = useMeasuredWidth(width);
  const [hover, setHover] = useState<{ ev: RibbonEvent; x: number; y: number } | null>(null);
  const [collapsed, setCollapsed] = usePersistedFlag("os.ribbon.collapsed", false);
  const [compact, setCompact] = usePersistedFlag("os.ribbon.compact", false);
  const LANE_ROW = compact ? LANE_ROW_COMPACT : LANE_ROW_FULL;
  const TOKEN_BAND = compact ? TOKEN_BAND_COMPACT : TOKEN_BAND_FULL;

  const w = Math.max(measuredW, PAD_L + PAD_R + 40);
  const lanes = model.lanes;
  const hasTokens = model.tokenSeries.length > 0;
  const bodyH = lanes.length * LANE_ROW;
  const tokenTop = PAD_T + bodyH + 6;
  const height = PAD_T + bodyH + (hasTokens ? TOKEN_BAND + 6 : 0) + AXIS_H + 8;

  // x scale — pad a zero-width domain so single-moment sessions still render.
  const [d0, d1] = model.domain;
  const domain: [Date, Date] =
    d1 > d0 ? [new Date(d0), new Date(d1)] : [new Date(d0 - 1000), new Date(d1 + 1000)];
  const x = scaleTime().domain(domain).range([PAD_L, w - PAD_R]);

  const laneY = (lane: LaneKey) => PAD_T + lanes.indexOf(lane) * LANE_ROW + LANE_ROW / 2;
  const r = scaleSqrt().domain([0, 20000]).range([2.5, 7]).clamp(true);

  // token burn area path
  const tokenPath = useMemo(() => {
    if (!hasTokens) return null;
    const maxTok = model.tokenSeries[model.tokenSeries.length - 1]?.cumulative || 1;
    const ty = scaleLinear().domain([0, maxTok]).range([tokenTop + TOKEN_BAND, tokenTop]);
    const gen = area<{ t: number; cumulative: number }>()
      .x((p) => x(new Date(p.t)))
      .y0(tokenTop + TOKEN_BAND)
      .y1((p) => ty(p.cumulative))
      .curve(curveMonotoneX);
    return gen(model.tokenSeries as { t: number; cumulative: number }[]);
  }, [hasTokens, model.tokenSeries, x, tokenTop]);

  const ticks = useMemo(() => x.ticks(Math.min(6, Math.max(2, Math.floor(w / 120)))), [x, w]);

  if (model.events.length === 0) {
    return (
      <div ref={containerRef} className={cn("px-3 py-4 text-[11px] text-[color:var(--text-muted)]", className)}>
        No activity to chart yet.
      </div>
    );
  }

  const axisY = height - AXIS_H;

  return (
    <div ref={containerRef} className={cn("relative w-full select-none", className)}>
      {/* Summary chips + view controls */}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 pt-2 pb-1 text-[10px] text-[color:var(--text-muted)]">
        <span className="text-[color:var(--text)]">{model.events.length} events</span>
        <span>· {humanDuration(model.durationMs)}</span>
        {model.totalTokens > 0 && (
          <span>· <span className="text-[color:var(--orange)]">{model.totalTokens.toLocaleString()}</span> out-tokens</span>
        )}
        {model.errorCount > 0 && (
          <span className="text-[color:var(--red)]">· {model.errorCount} error{model.errorCount > 1 ? "s" : ""}</span>
        )}
        <div className="ml-auto flex items-center gap-1.5">
          {!collapsed && (
            <button
              type="button"
              onClick={() => setCompact(!compact)}
              className="rounded border border-[color:var(--border)] px-2 py-0.5 text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
              title={compact ? "Taller lane rows" : "Shorter lane rows to save space"}
            >
              {compact ? "↕ full height" : "↕ compact"}
            </button>
          )}
          <button
            type="button"
            onClick={() => setCollapsed(!collapsed)}
            className="rounded border border-[color:var(--border)] px-2 py-0.5 text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
            title={collapsed ? "Show the activity timeline" : "Hide the activity timeline"}
            aria-expanded={!collapsed}
          >
            {collapsed ? "▸ show timeline" : "▾ hide"}
          </button>
        </div>
      </div>

      {!collapsed && (
      <svg width={w} height={height} className="block" role="img" aria-label="Session activity ribbon">
        {/* lane rows + labels */}
        {lanes.map((lane) => {
          const cy = laneY(lane);
          return (
            <g key={lane}>
              <line x1={PAD_L} x2={w - PAD_R} y1={cy} y2={cy} stroke="#2f3348" strokeWidth={1} />
              <text x={PAD_L - 8} y={cy} textAnchor="end" dominantBaseline="central" fontSize={10} fill="#565f89">
                {LANE_LABEL[lane]}
              </text>
            </g>
          );
        })}

        {/* token burn area */}
        {tokenPath && (
          <g>
            <path d={tokenPath} fill="#e0af68" fillOpacity={0.16} stroke="#e0af68" strokeOpacity={0.5} strokeWidth={1} />
            <text x={PAD_L - 8} y={tokenTop + TOKEN_BAND / 2} textAnchor="end" dominantBaseline="central" fontSize={9} fill="#565f89">
              tokens
            </text>
          </g>
        )}

        {/* event marks */}
        {model.events.map((ev) => {
          const cx = x(new Date(ev.t));
          const cy = laneY(ev.lane);
          const selected = selectedEventId === ev.id;
          return (
            <circle
              key={ev.id}
              data-ribbon-mark
              data-event-id={ev.id}
              cx={cx}
              cy={cy}
              r={selected ? r(ev.bytes) + 2 : r(ev.bytes)}
              fill={ev.color}
              fillOpacity={ev.isError ? 0.95 : 0.8}
              stroke={selected ? "#c0caf5" : ev.isError ? "#f7768e" : "none"}
              strokeWidth={selected ? 1.5 : ev.isError ? 1 : 0}
              className={onSelectEvent ? "cursor-pointer" : undefined}
              onMouseEnter={() => setHover({ ev, x: cx, y: cy })}
              onMouseLeave={() => setHover((h) => (h?.ev.id === ev.id ? null : h))}
              onClick={onSelectEvent ? () => onSelectEvent(ev.id) : undefined}
            >
              <title>{`${ev.label} — ${fmtFull(new Date(ev.t))}`}</title>
            </circle>
          );
        })}

        {/* time axis */}
        <line x1={PAD_L} x2={w - PAD_R} y1={axisY} y2={axisY} stroke="#2f3348" strokeWidth={1} />
        {ticks.map((tk, i) => {
          const tx = x(tk);
          return (
            <g key={i}>
              <line x1={tx} x2={tx} y1={axisY} y2={axisY + 4} stroke="#2f3348" strokeWidth={1} />
              <text x={tx} y={axisY + 14} textAnchor="middle" fontSize={9} fill="#565f89">
                {fmtAxis(tk)}
              </text>
            </g>
          );
        })}
      </svg>
      )}

      {/* hover tooltip */}
      {hover && (
        <div
          className="pointer-events-none absolute z-10 rounded border border-[color:var(--bg-hover)] bg-[color:var(--bg)] px-2 py-1 text-[10px] text-[color:var(--text)] shadow-lg"
          style={{ left: Math.min(hover.x + 8, w - 140), top: hover.y - 34 }}
        >
          <span className="font-medium" style={{ color: hover.ev.color }}>
            {hover.ev.label}
          </span>
          <span className="ml-1 text-[color:var(--text-muted)]">{fmtFull(new Date(hover.ev.t))}</span>
          {hover.ev.isError && <span className="ml-1 text-[color:var(--red)]">error</span>}
        </div>
      )}
    </div>
  );
}
