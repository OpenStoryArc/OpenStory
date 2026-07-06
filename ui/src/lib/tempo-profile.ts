/** Attention-aware pacing — the metronome's listening half.
 *
 *  An agent driving the dashboard should act in the human's RESTS, not over their
 *  shoulder. This reads the recent interaction stream and reports whether the
 *  user is active right now, how long they've been resting, and their cadence
 *  (the median gap between interactions — their rhythm). Pure: interactions +
 *  now in, a profile out; the caller (agent/MCP) decides to drive only when
 *  activeNow is false. */

/** Below this idle gap the user is considered actively engaged. */
export const IDLE_THRESHOLD_MS = 8_000;

export interface TempoProfile {
  /** True when the last interaction is within IDLE_THRESHOLD_MS of now. */
  readonly activeNow: boolean;
  /** Epoch ms of the most recent interaction, or null if none. */
  readonly lastActivityMs: number | null;
  /** How long the user has been resting (now - lastActivity); Infinity if none. */
  readonly restMs: number;
  /** Median inter-interaction gap over the window (the rhythm), or null if <2. */
  readonly cadenceMs: number | null;
}

function median(sorted: number[]): number {
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid]! : (sorted[mid - 1]! + sorted[mid]!) / 2;
}

/** Build the tempo profile from a list of interactions (each with an ISO `at`)
 *  relative to `nowMs`. Order-independent; unparseable timestamps are ignored. */
export function tempoProfile(interactions: readonly { at: string }[], nowMs: number): TempoProfile {
  const times = interactions
    .map((i) => new Date(i.at).getTime())
    .filter((t) => Number.isFinite(t))
    .sort((a, b) => a - b);

  if (times.length === 0) {
    return { activeNow: false, lastActivityMs: null, restMs: Infinity, cadenceMs: null };
  }

  const lastActivityMs = times[times.length - 1]!;
  const restMs = nowMs - lastActivityMs;
  const activeNow = restMs < IDLE_THRESHOLD_MS;

  let cadenceMs: number | null = null;
  if (times.length >= 2) {
    const gaps: number[] = [];
    for (let i = 1; i < times.length; i++) gaps.push(times[i]! - times[i - 1]!);
    cadenceMs = median(gaps.sort((a, b) => a - b));
  }

  return { activeNow, lastActivityMs, restMs, cadenceMs };
}
