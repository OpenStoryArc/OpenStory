import { describe, it, expect } from "vitest";
import { interactionScore, PRIMITIVES } from "@/lib/interaction-score";

/** The metronome's pure core: a musical "score" (Cool Cat by Queen — a laid-back
 *  4/4 groove) that the perf driver replays at escalating tempo to find knees.
 *  Each beat is a control primitive fired on the beat; tempo (bpm) scales gaps. */
describe("interactionScore — a timed sequence of control beats on the Cool Cat groove", () => {
  it("has bars*4 beats (quarter notes in 4/4)", () => {
    expect(interactionScore(120, 4)).toHaveLength(16);
    expect(interactionScore(90, 8)).toHaveLength(32);
  });

  it("places the first beat at 0 and spaces beats by 60000/bpm", () => {
    const s = interactionScore(120, 2); // 120bpm → 500ms/beat
    expect(s[0]!.atMs).toBe(0);
    expect(s[1]!.atMs).toBe(500);
    expect(s[7]!.atMs).toBe(3500);
  });

  it("tempo scales the gaps — double the bpm halves the beat spacing", () => {
    const slow = interactionScore(120, 2);
    const fast = interactionScore(240, 2);
    expect(fast[1]!.atMs).toBe(slow[1]!.atMs / 2);
    expect(fast[5]!.atMs).toBe(slow[5]!.atMs / 2);
  });

  it("exercises EVERY primitive across a full loop", () => {
    const actions = new Set(interactionScore(120, 4).map((b) => b.action));
    for (const p of PRIMITIVES) expect(actions.has(p)).toBe(true);
  });

  it("every beat carries an action and a params object", () => {
    for (const beat of interactionScore(120, 4)) {
      expect(typeof beat.action).toBe("string");
      expect(beat.params).toBeTypeOf("object");
      expect(beat.params).not.toBeNull();
    }
  });

  it("is deterministic — same args produce the same score", () => {
    expect(interactionScore(120, 4)).toEqual(interactionScore(120, 4));
  });
});
