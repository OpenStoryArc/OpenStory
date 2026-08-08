/** Pure model for the efficiency scatter — events × output-tokens in log-log
 *  space with an OLS fit line, so sessions OFF the regression (unusually
 *  productive or wasteful) pop as outliers. Zero-output sessions (uninstrumented
 *  agents like openactor/codex) are flagged for a separate gutter, not dropped.
 *  Side-effect-free so the regression math is unit-tested. */

import type { StorySession } from "@/lib/story-api";

export interface ScatterPoint {
  readonly id: string;
  readonly label: string;
  readonly events: number;
  readonly tokens: number; // output tokens ("work produced")
  readonly durationMs: number;
  readonly agent: string;
  /** true when tokens<=0 (no token telemetry) → rendered in the zero gutter. */
  readonly zero: boolean;
}

export interface ScatterFit {
  /** log10(tokens) = slope*log10(events) + intercept */
  readonly slope: number;
  readonly intercept: number;
  /** residual std-dev in log10 space (the ±band). */
  readonly sigma: number;
  readonly n: number;
}

export interface ScatterModel {
  readonly points: ScatterPoint[];
  readonly fit: ScatterFit | null;
}

function durationMs(s: StorySession): number {
  if (!s.start_time || !s.last_event) return 0;
  const a = Date.parse(s.start_time), b = Date.parse(s.last_event);
  return Number.isFinite(a) && Number.isFinite(b) ? Math.max(0, b - a) : 0;
}

/** Ordinary least squares on (log10 events, log10 tokens) over non-zero points. */
export function fitLogLog(points: readonly ScatterPoint[]): ScatterFit | null {
  const pts = points.filter((p) => p.events > 0 && p.tokens > 0);
  if (pts.length < 3) return null;
  const xs = pts.map((p) => Math.log10(p.events));
  const ys = pts.map((p) => Math.log10(p.tokens));
  const n = pts.length;
  const mx = xs.reduce((a, b) => a + b, 0) / n;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  let sxx = 0, sxy = 0;
  for (let i = 0; i < n; i++) { sxx += (xs[i]! - mx) ** 2; sxy += (xs[i]! - mx) * (ys[i]! - my); }
  const slope = sxx === 0 ? 0 : sxy / sxx;
  const intercept = my - slope * mx;
  let ss = 0;
  for (let i = 0; i < n; i++) { const r = ys[i]! - (slope * xs[i]! + intercept); ss += r * r; }
  const sigma = Math.sqrt(ss / Math.max(1, n - 2));
  return { slope, intercept, sigma, n };
}

/** A rectangular selection expressed in DATA space (not pixels), so the filter
 *  is scale-independent and unit-testable. `includeZero` admits uninstrumented
 *  (0-token) points when the brush covers the gutter column. */
export interface BrushExtent {
  readonly ev0: number;
  readonly ev1: number;
  readonly tok0: number;
  readonly tok1: number;
  readonly includeZero: boolean;
}

/** Points falling inside a brushed data window, most-productive (highest output
 *  tokens) first. Zero-token points only qualify when `includeZero` is set AND
 *  they fall in the events window. Pure — the linked-list panel renders this. */
export function pointsInBrush(points: readonly ScatterPoint[], b: BrushExtent): ScatterPoint[] {
  const [ev0, ev1] = b.ev0 <= b.ev1 ? [b.ev0, b.ev1] : [b.ev1, b.ev0];
  const [tok0, tok1] = b.tok0 <= b.tok1 ? [b.tok0, b.tok1] : [b.tok1, b.tok0];
  return points
    .filter((p) => {
      if (p.events < ev0 || p.events > ev1) return false;
      if (p.zero) return b.includeZero;
      return p.tokens >= tok0 && p.tokens <= tok1;
    })
    .sort((a, z) => z.tokens - a.tokens || z.events - a.events || (a.id < z.id ? -1 : 1));
}

/**
 * ScatterView paint state from Attention's data-space brush.
 * Agent drive path: foldIntent → canvas.scatterBrush → this → sink.
 * No control$ inject required — canvasAttention$ is enough.
 */
export function scatterPaintFromBrush(
  points: readonly ScatterPoint[],
  brush: BrushExtent | null | undefined,
): { selecting: boolean; brushed: ScatterPoint[] | null } {
  if (!brush) return { selecting: false, brushed: null };
  return { selecting: true, brushed: pointsInBrush(points, brush) };
}

/** Deterministic jitter offset within a disk of `radius` px, derived from the
 *  session id. Used to declutter overplotted points — dozens of 1-event sessions
 *  otherwise stack on the y-axis at x=1 and read as a rendering glitch. Uniform
 *  over the disk (sqrt on the radius) so the spread looks even, not clumped at
 *  the center. Pure → tested. */
export function pointJitter(id: string, radius: number): { dx: number; dy: number } {
  if (radius <= 0) return { dx: 0, dy: 0 };
  // Two independent hashes (different seeds/multipliers) so angle and radius
  // don't correlate for short, similar ids like "s0".."s49".
  let h1 = 2166136261, h2 = 5381;
  for (let i = 0; i < id.length; i++) {
    const c = id.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 16777619);
    h2 = (h2 * 33 + c) | 0;
  }
  const angle = ((h1 >>> 0) / 4294967296) * 2 * Math.PI;
  const r = radius * Math.sqrt(((h2 >>> 0) / 4294967296));
  return { dx: Math.cos(angle) * r, dy: Math.sin(angle) * r };
}

export interface ScatterOutlier {
  readonly point: ScatterPoint;
  /** residual in log10 space: actual − expected. >0 = above the line. */
  readonly residual: number;
}

/** The N points furthest ABOVE the fit line — the most output per event, i.e.
 *  "punching above their weight". Non-zero points only, most-productive first.
 *  Pure → the plot LABELS these so the chart's story is named, not just implied. */
export function scatterOutliers(
  points: readonly ScatterPoint[],
  fit: ScatterFit | null,
  n: number,
): ScatterOutlier[] {
  if (!fit || n <= 0) return [];
  return points
    .filter((p) => p.events > 0 && p.tokens > 0)
    .map((p) => ({ point: p, residual: Math.log10(p.tokens) - (fit.slope * Math.log10(p.events) + fit.intercept) }))
    .sort((a, b) => b.residual - a.residual)
    .slice(0, n);
}

export function buildScatter(sessions: readonly StorySession[]): ScatterModel {
  const points: ScatterPoint[] = sessions.map((s) => {
    const tokens = s.total_output_tokens ?? 0;
    return {
      id: s.session_id,
      label: s.label || s.session_id.slice(0, 8),
      events: s.event_count ?? 0,
      tokens,
      durationMs: durationMs(s),
      agent: s.origin_agent || "unknown",
      zero: tokens <= 0,
    };
  });
  return { points, fit: fitLogLog(points) };
}
