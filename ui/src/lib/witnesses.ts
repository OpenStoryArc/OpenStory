/** The Lab's witness runners — the falsifiable method made executable. Each
 *  candidate's NL `witness` string (in viz-candidates.json) is the SPEC; here it
 *  becomes a pure, tested function over the live sessions list that returns a
 *  verdict. A shape can't graduate to "buildable" until its witness fires green.
 *  Runners that need record-level data (tool transitions, precise parent
 *  resolution) are intentionally absent → runWitness returns null ("needs
 *  records"), which is honest rather than faking a green. */

import type { StorySession } from "@/lib/story-api";

export interface WitnessResult {
  /** did the data clear the bar? */
  readonly grounded: boolean;
  /** the measured quantity. */
  readonly value: number;
  /** what it needed to clear. */
  readonly threshold: number;
  /** human phrase for the badge. */
  readonly detail: string;
}

const durationMs = (s: StorySession): number => {
  if (!s.start_time || !s.last_event) return 0;
  const a = Date.parse(s.start_time), b = Date.parse(s.last_event);
  return Number.isFinite(a) && Number.isFinite(b) ? Math.max(0, b - a) : 0;
};
const projOf = (s: StorySession): string => s.project_name || s.project_id || "";
const isSub = (s: StorySession): boolean => s.session_id.startsWith("agent-");
const dayOf = (s: StorySession): string => (s.start_time ?? "").slice(0, 10);

/** Pearson correlation; <3 points → treat as 1 (correlated) so we don't claim independence on no evidence. */
export function pearson(xs: readonly number[], ys: readonly number[]): number {
  const n = Math.min(xs.length, ys.length);
  if (n < 3) return 1;
  const mx = xs.reduce((a, b) => a + b, 0) / n;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  let sxy = 0, sxx = 0, syy = 0;
  for (let i = 0; i < n; i++) { const dx = xs[i]! - mx, dy = ys[i]! - my; sxy += dx * dy; sxx += dx * dx; syy += dy * dy; }
  const d = Math.sqrt(sxx * syy);
  return d === 0 ? 1 : sxy / d;
}

const subagentWitness = (ss: readonly StorySession[]): WitnessResult => {
  const n = ss.filter(isSub).length;
  return { grounded: n >= 10, value: n, threshold: 10, detail: `${n} agent-* subagent sessions (parent-resolution needs records)` };
};

export const WITNESS_RUNNERS: Record<string, (ss: readonly StorySession[]) => WitnessResult> = {
  "delegation-graph": subagentWitness,
  "delegation-tree": subagentWitness,
  "delegation-arc-diagram": subagentWitness,

  "agent-project-matrix": (ss) => {
    const agents = new Set(ss.map((s) => s.origin_agent).filter(Boolean)).size;
    const projects = new Set(ss.map(projOf).filter(Boolean)).size;
    return { grounded: agents >= 2 && projects >= 3, value: agents * projects, threshold: 6, detail: `${agents} agents × ${projects} projects` };
  },

  "agent-project-chord": (ss) => {
    const pairs = new Set<string>();
    const counts = new Map<string, number>();
    for (const s of ss) { const k = `${s.origin_agent}|${projOf(s)}`; counts.set(k, (counts.get(k) ?? 0) + 1); }
    for (const [k, c] of counts) if (c > 1) pairs.add(k);
    return { grounded: pairs.size >= 6, value: pairs.size, threshold: 6, detail: `${pairs.size} agent×project pairs with >1 session` };
  },

  "duration-beeswarm": (ss) => {
    const withDur = ss.filter((s) => durationMs(s) > 0);
    const agents = new Set(withDur.map((s) => s.origin_agent).filter(Boolean)).size;
    const ds = withDur.map(durationMs);
    const decades = ds.length ? Math.log10(Math.max(...ds) / Math.max(1, Math.min(...ds))) : 0;
    const dec = Math.round(decades * 10) / 10;
    return { grounded: dec >= 2 && agents >= 2, value: dec, threshold: 2, detail: `${dec} decades of duration across ${agents} agents` };
  },

  "events-streamgraph": (ss) => {
    const days = new Set(ss.map(dayOf).filter((d) => d.length === 10)).size;
    const agents = new Set(ss.map((s) => s.origin_agent).filter(Boolean)).size;
    return { grounded: days >= 7 && agents >= 2, value: days, threshold: 7, detail: `${days} active days across ${agents} agents` };
  },

  "session-parallel-coords": (ss) => {
    const ev = ss.map((s) => s.event_count ?? 0);
    const tok = ss.map((s) => s.total_output_tokens ?? 0);
    const dur = ss.map(durationMs);
    const rs = [Math.abs(pearson(ev, tok)), Math.abs(pearson(ev, dur)), Math.abs(pearson(tok, dur))];
    const minR = Math.round(Math.min(...rs) * 100) / 100;
    return { grounded: minR < 0.8, value: minR, threshold: 0.8, detail: `weakest |r|=${minR} (<0.8 ⇒ axes independent enough to cross)` };
  },

  "fleet-force-graph": (ss) => {
    const dims = [
      new Set(ss.map((s) => s.principal_id).filter(Boolean)).size,
      new Set(ss.map((s) => s.host).filter(Boolean)).size,
      new Set(ss.map((s) => s.origin_agent).filter(Boolean)).size,
    ];
    const withVariety = dims.filter((d) => d >= 2).length;
    return { grounded: withVariety >= 2, value: withVariety, threshold: 2, detail: `${withVariety}/3 fleet dims vary (principal/host/agent)` };
  },

  "token-hexbin": (ss) => ({ grounded: ss.length >= 500, value: ss.length, threshold: 500, detail: `${ss.length} sessions in the events×tokens plane` }),

  "session-radar": (ss) => ({ grounded: ss.length >= 3, value: ss.length, threshold: 3, detail: `${ss.length} sessions to fingerprint` }),
};

/** Run a candidate's witness over the sessions, or null if there's no runner yet
 *  (e.g. it needs record-level data we don't fetch here). */
export function runWitness(candidateId: string, sessions: readonly StorySession[]): WitnessResult | null {
  const fn = WITNESS_RUNNERS[candidateId];
  return fn ? fn(sessions) : null;
}
