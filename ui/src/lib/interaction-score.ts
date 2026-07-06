/** The METRONOME's pure core — a musical "score" for the perf harness.
 *
 *  Cool Cat (Queen, 1982) is a laid-back 4/4 funk groove: kick on 1 & 3, snare
 *  backbeat on 2 & 4, hats filling the offbeats. We map that groove to the
 *  agent-in-UI control primitives so replaying the score DRIVES the dashboard on
 *  the beat — and replaying it at escalating tempo (bpm) is how the driver finds
 *  performance knees. Pure: bpm + bars in, a timed beat sequence out; the driver
 *  (scripts/metronome_drive.mjs) handles the POSTs and the stopwatch. */

/** The control primitives the harness exercises — every one must appear in a
 *  full loop so the perf sweep covers the whole write surface. `interaction`
 *  targets POST /api/interactions; the rest target POST /api/control. */
export const PRIMITIVES = [
  "open_view",
  "focus_event",
  "toggle",
  "query",
  "present",
  "interaction",
] as const;

export type Primitive = (typeof PRIMITIVES)[number];

export interface Beat {
  /** Milliseconds from score start (beatIndex * 60000/bpm). */
  readonly atMs: number;
  /** Which primitive fires on this beat. */
  readonly action: Primitive;
  /** Ready-to-POST params for the primitive. */
  readonly params: Record<string, unknown>;
}

/** The Cool Cat groove as a repeating 8-beat (2-bar) riff of primitives. The
 *  kick/snare backbeat (open_view / focus_event alternating like 1&3 / 2&4) with
 *  the hats/ghost notes carrying the fills (toggle, query, present, interaction).
 *  All six primitives appear within the riff, so any loop ≥ 2 bars covers them. */
const RIFF: readonly Primitive[] = [
  "open_view", // 1 — kick
  "toggle", //     & — hat
  "focus_event", // 2 — snare (backbeat)
  "query", //      & — hat
  "open_view", // 3 — kick
  "present", //    & — hat
  "focus_event", // 4 — snare (backbeat)
  "interaction", // & — ghost note
];

/** Rotating, deterministic param pools so each fired primitive is valid and the
 *  targets vary across the loop (no randomness — the score is reproducible). */
const VIEWS = ["canvas", "story", "explore", "lab"];
const MODES = ["board", "sunburst", "treemap", "scatter", "gantt", "flow"];
const FACETS = ["project", "agent", "status", "host"];
const SESSIONS = ["demo-a", "demo-b", "demo-c"];

function paramsFor(primitive: Primitive, i: number): Record<string, unknown> {
  switch (primitive) {
    case "open_view":
      return { view: VIEWS[i % VIEWS.length] };
    case "focus_event":
      return { sessionId: SESSIONS[i % SESSIONS.length], eventId: `evt-${i}`, view: "explore" };
    case "toggle":
      return { target: "canvas.mode", value: MODES[i % MODES.length] };
    case "query":
      return { [FACETS[i % FACETS.length]!]: `probe-${i}` };
    case "present":
      return { message: `metronome beat ${i}`, issuer: "metronome" };
    case "interaction":
      return { kind: "navigate", view: VIEWS[i % VIEWS.length], issuer: "metronome" };
  }
}

/** Build the score: `bars` bars of 4/4 at `bpm`. Beat i fires RIFF[i % 8] at
 *  `i * 60000/bpm` ms. Higher bpm ⇒ tighter beats ⇒ more load per second. */
export function interactionScore(bpm: number, bars: number): Beat[] {
  const beatMs = 60000 / (bpm > 0 ? bpm : 1);
  const total = Math.max(0, Math.floor(bars)) * 4;
  const beats: Beat[] = [];
  for (let i = 0; i < total; i++) {
    const action = RIFF[i % RIFF.length]!;
    beats.push({ atMs: i * beatMs, action, params: paramsFor(action, i) });
  }
  return beats;
}
