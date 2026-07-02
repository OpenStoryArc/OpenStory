import { describe, it, expect } from "vitest";
import { tempoProfile, IDLE_THRESHOLD_MS } from "@/lib/tempo-profile";

const NOW = Date.parse("2026-07-02T14:00:00Z");
const at = (msAgo: number) => ({ at: new Date(NOW - msAgo).toISOString() });

/** Attention-aware pacing: read the human's recent interaction rhythm to know
 *  whether they're ACTIVE now or RESTING — so an agent acts in the gaps. */
describe("tempoProfile — detect the user's rests from their interaction rhythm", () => {
  it("empty stream → not active, no last activity, huge rest", () => {
    const t = tempoProfile([], NOW);
    expect(t.activeNow).toBe(false);
    expect(t.lastActivityMs).toBeNull();
    expect(t.cadenceMs).toBeNull();
    expect(t.restMs).toBe(Infinity);
  });

  it("recent activity (within the idle threshold) → active now", () => {
    const t = tempoProfile([at(30_000), at(2_000)], NOW);
    expect(t.activeNow).toBe(true);
    expect(t.restMs).toBe(2_000);
    expect(t.lastActivityMs).toBe(NOW - 2_000);
  });

  it("a long gap since the last interaction → resting", () => {
    const t = tempoProfile([at(60_000), at(20_000)], NOW);
    expect(t.activeNow).toBe(false); // 20s > IDLE_THRESHOLD (8s)
    expect(t.restMs).toBe(20_000);
  });

  it("cadence is the median inter-interaction gap (the rhythm)", () => {
    // gaps between consecutive interactions: 1s, 3s, 1s → median 1s
    const t = tempoProfile([at(6_000), at(5_000), at(2_000), at(1_000)], NOW);
    expect(t.cadenceMs).toBe(1_000);
  });

  it("ignores interactions with an unparseable timestamp", () => {
    const t = tempoProfile([{ at: "nope" }, at(1_000)], NOW);
    expect(t.activeNow).toBe(true);
    expect(t.restMs).toBe(1_000);
  });

  it("IDLE_THRESHOLD_MS is exported and positive", () => {
    expect(IDLE_THRESHOLD_MS).toBeGreaterThan(0);
  });
});
