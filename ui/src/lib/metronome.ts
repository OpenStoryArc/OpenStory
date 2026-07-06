/** Metronome — the rhythm engine behind the perf-harness (replay UI activity at
 *  a musical tempo) and, later, attention-aware pacing (act in the user's rests,
 *  not over their thoughts). Two pure halves:
 *   - `musicalScore(params)` turns tempo + a beat pattern into timed hits — a
 *     "piano roll" whose SHAPE is real (bursts, ghost notes, phrase rests),
 *     unlike a uniform firehose.
 *   - `tempoProfile(timestamps)` reads a rhythm back out of an event stream
 *     (e.g. the user's `openstory-ui` interactions) → their tempo.
 *  Side-effect-free → unit-tested; the I/O driver that replays a score against
 *  the server lives in scripts/. */

export interface Beat {
  /** offset from the score's start, in ms. */
  readonly tMs: number;
  /** velocity 0..1 — 1 is a down-beat, <1 a ghost note (syncopation). */
  readonly strength: number;
}

export interface ScoreParams {
  bpm: number;
  bars: number;
  /** beats per bar (time signature numerator). Default 4. */
  beatsPerBar?: number;
  /** per-step velocity across ONE bar; 0 = rest. Length = the step grid
   *  (e.g. 16 → sixteenth notes). Default a bare four-on-the-floor kick. */
  pattern?: number[];
  /** 0..1 — pushes off-beat (odd) steps later for a swung feel. Default 0. */
  swing?: number;
  /** insert a silent bar after every N content bars (phrase breathing). */
  restEveryBars?: number;
}

/** BPM → quarter-note interval in ms. */
export function beatIntervalMs(bpm: number): number {
  return 60000 / bpm;
}

/** Render a beat pattern into timed, velocity-carrying hits. */
export function musicalScore(p: ScoreParams): Beat[] {
  const beatsPerBar = p.beatsPerBar ?? 4;
  const pattern = p.pattern ?? [1, 0, 0, 0];
  const swing = p.swing ?? 0;
  const restEveryBars = p.restEveryBars ?? 0;
  const barMs = beatIntervalMs(p.bpm) * beatsPerBar;
  const steps = pattern.length;
  const stepMs = barMs / steps;

  const beats: Beat[] = [];
  let cursor = 0;
  for (let bar = 0; bar < p.bars; bar++) {
    for (let i = 0; i < steps; i++) {
      const v = pattern[i]!;
      if (v > 0) {
        const swingOffset = i % 2 === 1 ? swing * stepMs * 0.5 : 0;
        beats.push({ tMs: cursor + i * stepMs + swingOffset, strength: v });
      }
    }
    cursor += barMs;
    if (restEveryBars > 0 && (bar + 1) % restEveryBars === 0) cursor += barMs; // phrase rest
  }
  return beats;
}

/** Read a rhythm out of a series of event timestamps (ms): the gaps between
 *  events and the median tempo they imply. This is how we measure the user's
 *  cadence from their interaction stream. */
export function tempoProfile(timestampsMs: readonly number[]): { intervals: number[]; medianBpm: number | null } {
  const intervals: number[] = [];
  for (let i = 1; i < timestampsMs.length; i++) intervals.push(timestampsMs[i]! - timestampsMs[i - 1]!);
  if (intervals.length === 0) return { intervals, medianBpm: null };
  const sorted = [...intervals].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const median = sorted.length % 2 ? sorted[mid]! : (sorted[mid - 1]! + sorted[mid]!) / 2;
  return { intervals, medianBpm: median > 0 ? Math.round(60000 / median) : null };
}

/** "Cool Cat" (Queen, Hot Space) — a loose interpretation of its laid-back,
 *  syncopated funk feel as a 16th-note groove: strong down-beats, ghost notes
 *  on the off-beats, a touch of swing. Not a transcription — a rhythm with the
 *  right *shape* to drive/replay the UI to a beat. */
export const COOL_CAT: ScoreParams = {
  bpm: 108,
  bars: 8,
  beatsPerBar: 4,
  swing: 0.3,
  pattern: [1, 0, 0.4, 0, 0.7, 0, 0.5, 0.3, 0.9, 0, 0.4, 0, 0.6, 0, 0.5, 0.35],
};
