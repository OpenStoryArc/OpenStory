import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildScatter, fitLogLog } from "@/lib/sessions-scatter";
import type { StorySession } from "@/lib/story-api";

function sess(id: string, events: number, out: number, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, event_count: events, total_output_tokens: out, origin_agent: "claude-code", status: "completed", ...over };
}

describe("fitLogLog", () => {
  it("recovers the slope/intercept of a perfect power law", () => {
    scenario(
      // tokens = 10 * events^2  ⇒ log10(tok) = 2*log10(ev) + 1
      () => [10, 20, 50, 100].map((e, i) => ({ id: `p${i}`, label: "", events: e, tokens: 10 * e * e, durationMs: 0, agent: "a", zero: false })),
      (pts) => fitLogLog(pts)!,
      (fit) => {
        expect(fit.slope).toBeCloseTo(2, 5);
        expect(fit.intercept).toBeCloseTo(1, 5);
        expect(fit.sigma).toBeCloseTo(0, 5);
        expect(fit.n).toBe(4);
      },
    );
  });

  it("ignores zero-token / zero-event points and returns null when too few", () => {
    expect(fitLogLog([{ id: "z", label: "", events: 5, tokens: 0, durationMs: 0, agent: "a", zero: true }])).toBeNull();
  });
});

describe("buildScatter", () => {
  it("maps sessions to points and flags zero-output ones", () => {
    scenario(
      () => buildScatter([sess("a", 100, 5000, { origin_agent: "claude-code" }), sess("b", 50, 0, { origin_agent: "openactor" })]),
      (m) => m,
      (m) => {
        const a = m.points.find((p) => p.id === "a")!;
        expect(a.events).toBe(100);
        expect(a.tokens).toBe(5000);
        expect(a.zero).toBe(false);
        expect(m.points.find((p) => p.id === "b")!.zero).toBe(true);
      },
    );
  });

  it("computes a fit when there are enough non-zero points", () => {
    scenario(
      () => buildScatter([sess("a", 10, 100), sess("b", 20, 400), sess("c", 40, 1600)]),
      (m) => m.fit,
      (fit) => { expect(fit).not.toBeNull(); expect(fit!.slope).toBeGreaterThan(0); },
    );
  });
});
