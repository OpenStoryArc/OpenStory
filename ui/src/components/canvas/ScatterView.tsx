/** ScatterView — the efficiency scatter: events × output-tokens in log-log
 *  space with an OLS fit line + ±1σ band, so sessions off the regression pop as
 *  outliers. Size = duration, color = agent. Zero-output sessions
 *  (uninstrumented agents) sit in a labeled gutter rather than being dropped.
 *  Click a point → open its session. Pure model + fit in lib/sessions-scatter. */

import { useEffect, useMemo, useRef, useState } from "react";
import { controlActions$ } from "@/streams/control";
import { canvasAttention$ } from "@/streams/attention";
import { scaleLog, scaleSqrt } from "d3-scale";
import { brush as d3brush } from "d3-brush";
import { select } from "d3-selection";
import type { StorySession } from "@/lib/story-api";
import {
  buildScatter,
  pointsInBrush,
  pointJitter,
  scatterOutliers,
  scatterPaintFromBrush,
  type ScatterPoint,
} from "@/lib/sessions-scatter";
import { agentColor } from "@/lib/agent-color";
import { AgentLegend } from "./AgentLegend";
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
  const [brushed, setBrushed] = useState<ScatterPoint[] | null>(null);
  const [selecting, setSelecting] = useState(false);

  // Prefer Attention (canvasAttention$): navigate_to / foldSteps commits
  // scatterBrush first; materializeAttention does not dual-inject. control$
  // remains for direct set scatter.brush (WS / sequence injectControl hops).
  useEffect(() => {
    const sub = canvasAttention$().subscribe((c) => {
      if (c.scatterBrush === undefined) return;
      const paint = scatterPaintFromBrush(model.points, c.scatterBrush);
      setSelecting(paint.selecting);
      setBrushed(paint.brushed);
    });
    return () => sub.unsubscribe();
  }, [model]);

  // control$ fallback: direct set scatter.brush (not via Attention fold).
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type !== "set" || a.target !== "scatter.brush") return;
      const p = a.params as Record<string, unknown>;
      const num = (v: unknown, d: number) => (Number.isFinite(Number(v)) ? Number(v) : d);
      const paint = scatterPaintFromBrush(model.points, {
        ev0: num(p.ev0, 1),
        ev1: num(p.ev1, Number.MAX_SAFE_INTEGER),
        tok0: num(p.tok0, 0),
        tok1: num(p.tok1, Number.MAX_SAFE_INTEGER),
        includeZero: p.includeZero === true,
      });
      setSelecting(paint.selecting);
      setBrushed(paint.brushed);
    });
    return () => sub.unsubscribe();
  }, [model]);

  const plotLeft = M.left + GUTTER_W;
  const plotRight = width - M.right;
  const plotTop = M.top;
  const plotBottom = height - M.bottom;

  const maxEv = Math.max(10, ...model.points.map((p) => p.events));
  const maxTok = Math.max(10, ...model.points.map((p) => p.tokens));
  const x = useMemo(() => scaleLog().domain([1, maxEv]).range([plotLeft, plotRight]).clamp(true), [maxEv, plotLeft, plotRight]);
  const y = useMemo(() => scaleLog().domain([1, maxTok]).range([plotBottom, plotTop]).clamp(true), [maxTok, plotBottom, plotTop]);

  // 2-D brush over the log-log cloud → linked list of the selected sessions.
  // Selection is in pixels; convert to a DATA-space extent so the tested pure
  // filter (pointsInBrush) decides membership. Gutter/zero points stay out.
  const brushRef = useRef<SVGGElement | null>(null);
  useEffect(() => {
    const g = brushRef.current;
    if (!g || !selecting) return;
    const b = d3brush<unknown>()
      .extent([[plotLeft, plotTop], [plotRight, plotBottom]])
      .on("brush end", (ev) => {
        const sel = ev.selection as [[number, number], [number, number]] | null;
        if (!sel) { setBrushed(null); return; }
        const [[x0, y0], [x1, y1]] = sel;
        const hits = pointsInBrush(model.points, {
          ev0: x.invert(x0), ev1: x.invert(x1),
          tok0: y.invert(y1), tok1: y.invert(y0), // y is inverted (top = high)
          includeZero: false,
        });
        setBrushed(hits);
      });
    const sel = select(g);
    sel.call(b);
    return () => { sel.on(".brush", null); sel.selectAll("*").remove(); };
  }, [model, x, y, plotLeft, plotRight, plotTop, plotBottom, selecting]);

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

  // Name the sessions punching furthest above the line (the story) + count the
  // uninstrumented gutter so it reads as "no telemetry", not noise.
  const outliers = useMemo(() => scatterOutliers(model.points, fit, 4), [model, fit]);
  const zeroCount = useMemo(() => model.points.filter((p) => p.zero).length, [model]);

  const xTicks = x.ticks(4);
  const yTicks = y.ticks(4);
  const kfmt = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(0)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(0)}k` : String(n));

  return (
    <div className="relative min-h-0 flex-1 bg-[color:var(--bg)]">
      <AgentLegend agents={model.points.map((p) => p.agent)} className="absolute left-3 top-2 z-10" />
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

        {/* zero / uninstrumented gutter — explained, so the wall reads as "no
            telemetry" rather than noise. */}
        <rect x={M.left} y={plotTop} width={GUTTER_W} height={plotBottom - plotTop} fill="#565f89" fillOpacity={0.05} />
        <text x={M.left + GUTTER_W / 2} y={plotBottom + 15} textAnchor="middle" fontSize={8} fill="#565f89">0 tok</text>
        {zeroCount > 0 && (
          <text transform={`translate(${M.left + GUTTER_W / 2},${(plotTop + plotBottom) / 2}) rotate(-90)`} textAnchor="middle" fontSize={8} fill="#565f89" className="pointer-events-none select-none">
            {zeroCount} sessions · no token telemetry
          </text>
        )}

        {/* fit line + ±1σ band + the ON-PLOT STORY: what the line means. */}
        {fitPath && (
          <g>
            <path d={fitPath.hi} fill="none" stroke="#565f89" strokeOpacity={0.35} strokeDasharray="3 3" />
            <path d={fitPath.lo} fill="none" stroke="#565f89" strokeOpacity={0.35} strokeDasharray="3 3" />
            <path d={fitPath.mid} fill="none" stroke="#c0caf5" strokeOpacity={0.5} strokeWidth={1.4} />
            <text x={(plotLeft + plotRight) / 2} y={plotTop + 11} textAnchor="middle" fontSize={10} fill="#a9b1d6" className="pointer-events-none select-none">
              the line = expected output · dots above it produce more per event ↑
            </text>
          </g>
        )}

        {/* points */}
        {model.points.map((p) => {
          // Non-zero points get a small deterministic jitter so the ~73 1-event
          // sessions don't stack on the y-axis; clamp so jitter never pushes a
          // point into the gutter or off-plot.
          const j = pointJitter(p.id, p.zero ? GUTTER_W / 2 - 4 : 5);
          const px = p.zero
            ? M.left + GUTTER_W / 2 + j.dx // spread across the gutter width, not one line
            : Math.min(plotRight, Math.max(plotLeft, x(Math.max(1, p.events)) + j.dx));
          const py = p.zero
            ? plotTop + hashJitter(p.id) * (plotBottom - plotTop)
            : Math.min(plotBottom, Math.max(plotTop, y(Math.max(1, p.tokens)) + j.dy));
          const inBrush = brushed?.some((b) => b.id === p.id);
          const dim = selecting && brushed != null && !inBrush;
          return (
            <circle
              key={p.id} data-scatter-point={p.id}
              cx={px} cy={py} r={rSize(p.durationMs)} fill={agentColor(p.agent)} fillOpacity={dim ? 0.12 : 0.62}
              stroke={inBrush ? "#c0caf5" : "none"} strokeWidth={inBrush ? 1 : 0}
              className={selecting ? "" : "cursor-pointer"}
              onMouseEnter={selecting ? undefined : () => setHover({ p, x: px, y: py })}
              onMouseLeave={selecting ? undefined : () => setHover((h) => (h?.p.id === p.id ? null : h))}
              onClick={selecting ? undefined : () => onOpenSession(p.id)}
            />
          );
        })}

        {/* name the most-productive outliers (furthest above the line) so the
            story is told, not just implied. Gold + haloed for legibility. */}
        {!selecting && outliers.map(({ point: p }) => {
          const px = Math.min(plotRight - 4, Math.max(plotLeft, x(Math.max(1, p.events))));
          const py = Math.min(plotBottom, Math.max(plotTop, y(Math.max(1, p.tokens))));
          return (
            <text
              key={`ol-${p.id}`} x={px + 7} y={py + 3} fontSize={9} fontWeight={600} fill="#e0af68"
              className="pointer-events-none select-none" style={{ paintOrder: "stroke" }} stroke="#16171f" strokeWidth={2.5}
            >
              {cleanHarnessPreview(p.label).slice(0, 16)}
            </text>
          );
        })}

        {/* marquee brush layer — only mounted in Select mode so it doesn't steal
            clicks from points otherwise. */}
        {selecting && <g ref={brushRef} data-testid="scatter-brush" />}
      </svg>

      {/* Select-mode toggle */}
      <button
        onClick={() => { setSelecting((s) => !s); setBrushed(null); setHover(null); }}
        className={`absolute right-3 top-3 z-10 rounded border px-2 py-1 text-[10px] transition-colors ${
          selecting ? "border-[color:var(--accent)] bg-[color:var(--accent)] text-[color:var(--bg)]" : "border-[color:var(--divider)] bg-[color:var(--bg)] text-[color:var(--text-bright)] hover:border-[color:var(--accent)]"
        }`}
        title="Drag a box over the cloud to list those sessions"
      >
        {selecting ? "Selecting — drag a box" : "Select"}
      </button>

      {/* linked list of brushed sessions */}
      {selecting && brushed && brushed.length > 0 && (
        <div className="absolute bottom-3 right-3 z-10 max-h-[45%] w-64 overflow-y-auto rounded border border-[color:var(--divider)] bg-[color:var(--bg)]/95 p-2 shadow-xl">
          <div className="mb-1 flex items-center justify-between text-[10px] text-[color:var(--text-muted)]">
            <span>{brushed.length} session{brushed.length === 1 ? "" : "s"} selected</span>
            <span>out tok</span>
          </div>
          <div className="flex flex-col divide-y divide-[color:var(--bg-hover)]/60">
            {brushed.slice(0, 40).map((p) => (
              <button
                key={p.id}
                onClick={() => onOpenSession(p.id)}
                className="flex items-center gap-2 py-1 text-left hover:bg-[color:var(--bg-surface)]"
              >
                <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: agentColor(p.agent) }} />
                <span className="min-w-0 flex-1 truncate text-[11px] text-[color:var(--text)]">{cleanHarnessPreview(p.label)}</span>
                <span className="shrink-0 text-[10px] tabular-nums text-[color:var(--accent)]">{kfmt(p.tokens)}</span>
              </button>
            ))}
            {brushed.length > 40 && <div className="pt-1 text-[9px] text-[color:var(--text-muted)]">+{brushed.length - 40} more…</div>}
          </div>
        </div>
      )}

      {hover && (
        <div className="pointer-events-none absolute z-10 rounded border border-[color:var(--divider)] bg-[color:var(--bg)] px-2 py-1 text-[10px] text-[color:var(--text)] shadow-lg" style={{ left: Math.min(hover.x + 10, width - 160), top: hover.y - 8 }}>
          <div className="max-w-[150px] truncate">{cleanHarnessPreview(hover.p.label)}</div>
          <div className="text-[color:var(--text-muted)]">{hover.p.events} ev · {kfmt(hover.p.tokens)} out · {formatDuration(hover.p.durationMs)} · <span style={{ color: agentColor(hover.p.agent) }}>{hover.p.agent}</span></div>
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
