import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { beatIntervalMs, musicalScore, tempoProfile, COOL_CAT } from "@/lib/metronome";

describe("beatIntervalMs", () => {
  it("converts BPM to a quarter-note interval in ms", () => {
    scenario(
      () => [beatIntervalMs(120), beatIntervalMs(60)],
      (v) => v,
      ([a, b]) => { expect(a).toBe(500); expect(b).toBe(1000); },
    );
  });
});

describe("musicalScore — a rhythm shaped by a beat pattern", () => {
  it("emits a hit only where the pattern is non-zero, on a monotonic grid", () => {
    scenario(
      () => musicalScore({ bpm: 120, bars: 2, beatsPerBar: 4, pattern: [1, 0, 1, 0] }),
      (beats) => beats,
      (beats) => {
        // 2 hits/bar × 2 bars = 4; step = (500*4)/4 = 500ms
        expect(beats.map((b) => b.tMs)).toEqual([0, 1000, 2000, 3000]);
        expect(beats.every((b) => b.strength === 1)).toBe(true);
      },
    );
  });

  it("carries per-step velocity as strength (downbeat vs ghost note)", () => {
    scenario(
      () => musicalScore({ bpm: 120, bars: 1, beatsPerBar: 4, pattern: [1, 0, 0.5, 0] }),
      (beats) => beats.map((b) => b.strength),
      (str) => expect(str).toEqual([1, 0.5]),
    );
  });

  it("swings the off-beats later without moving the down-beats", () => {
    scenario(
      () => ({
        straight: musicalScore({ bpm: 120, bars: 1, beatsPerBar: 4, pattern: [1, 1, 1, 1], swing: 0 }),
        swung: musicalScore({ bpm: 120, bars: 1, beatsPerBar: 4, pattern: [1, 1, 1, 1], swing: 0.5 }),
      }),
      (r) => r,
      ({ straight, swung }) => {
        expect(swung[0]!.tMs).toBe(straight[0]!.tMs);   // downbeat unmoved
        expect(swung[1]!.tMs).toBeGreaterThan(straight[1]!.tMs); // offbeat pushed later
      },
    );
  });

  it("inserts a silent bar as a phrase rest", () => {
    scenario(
      () => musicalScore({ bpm: 120, bars: 2, beatsPerBar: 4, pattern: [1, 0, 0, 0], restEveryBars: 1 }),
      (beats) => beats.map((b) => b.tMs),
      // bar0 hit at 0; a rest bar (2000ms) inserted; bar1 hit at 4000, not 2000
      (ts) => expect(ts).toEqual([0, 4000]),
    );
  });
});

describe("tempoProfile — read a rhythm back out of timestamps", () => {
  it("computes inter-event intervals and a median BPM", () => {
    scenario(
      () => tempoProfile([0, 500, 1000, 1500]),
      (p) => p,
      (p) => { expect(p.intervals).toEqual([500, 500, 500]); expect(p.medianBpm).toBe(120); },
    );
  });

  it("is empty-safe", () => {
    scenario(
      () => tempoProfile([]),
      (p) => p,
      (p) => { expect(p.intervals).toEqual([]); expect(p.medianBpm).toBeNull(); },
    );
  });
});

describe("COOL_CAT preset", () => {
  it("is a laid-back, syncopated groove (a real, non-empty score)", () => {
    scenario(
      () => musicalScore(COOL_CAT),
      (beats) => beats,
      (beats) => {
        expect(beats.length).toBeGreaterThan(8);
        expect(beats.every((b, i) => i === 0 || b.tMs > beats[i - 1]!.tMs)).toBe(true);
        expect(beats.some((b) => b.strength < 1)).toBe(true); // ghost notes → syncopation
      },
    );
  });
});
