/** Pure model for Session Parallel Coordinates (Lab candidate
 *  `session-parallel-coords`). Each session becomes a polyline across a fixed
 *  set of numeric axes; each axis is normalized independently to [0,1] by its
 *  own min/max so incommensurable quantities (events vs tokens vs milliseconds)
 *  share a chart. A constant axis maps to 0.5 (no divide-by-zero). Multivariate
 *  outliers show as lines that cross the bundle. Side-effect-free →
 *  unit-tested; sessions-list only (no records). */

import { sessionDurationMs } from "@/lib/sessions-overview";
import type { StorySession } from "@/lib/story-api";

export interface PCAxisSpec {
  readonly key: string;
  readonly label: string;
  readonly value: (s: StorySession) => number;
}

export interface PCAxis { readonly key: string; readonly label: string }

export interface PCLine {
  readonly session_id: string;
  readonly agent: string;
  readonly label: string;
  readonly raw: number[];     // native per-axis values, axis order
  readonly coords: number[];  // normalized [0,1], axis order
}

export interface ParallelCoords {
  readonly axes: PCAxis[];
  readonly lines: PCLine[];
  readonly domains: [number, number][]; // per-axis [min,max] of raw values
}

/** Four honest axes derivable from the session list alone. */
export const DEFAULT_PC_AXES: readonly PCAxisSpec[] = [
  { key: "events", label: "Events", value: (s) => s.event_count ?? 0 },
  { key: "in", label: "Input tokens", value: (s) => s.total_input_tokens ?? 0 },
  { key: "out", label: "Output tokens", value: (s) => s.total_output_tokens ?? 0 },
  { key: "duration", label: "Duration", value: (s) => sessionDurationMs(s) },
];

export function buildParallelCoords(
  sessions: readonly StorySession[],
  { axes = DEFAULT_PC_AXES }: { axes?: readonly PCAxisSpec[] } = {},
): ParallelCoords {
  const axisOut: PCAxis[] = axes.map((a) => ({ key: a.key, label: a.label }));

  // per-axis raw values, then domains
  const raws = sessions.map((s) => axes.map((a) => a.value(s)));
  const domains: [number, number][] = axes.map((_, j) => {
    let min = Infinity, max = -Infinity;
    for (const r of raws) { const v = r[j]!; if (v < min) min = v; if (v > max) max = v; }
    return raws.length ? [min, max] : [0, 1];
  });

  const norm = (v: number, j: number) => {
    const [min, max] = domains[j]!;
    return max === min ? 0.5 : (v - min) / (max - min);
  };

  const lines: PCLine[] = sessions.map((s, i) => ({
    session_id: s.session_id,
    agent: s.origin_agent || "unknown",
    label: s.label || s.session_id.slice(0, 8),
    raw: raws[i]!,
    coords: raws[i]!.map((v, j) => norm(v, j)),
  }));

  return { axes: axisOut, lines, domains };
}
