import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { pointsInBrush, type ScatterPoint } from "@/lib/sessions-scatter";

function pt(id: string, events: number, tokens: number): ScatterPoint {
  return { id, label: id, events, tokens, durationMs: 0, agent: "claude-code", zero: tokens <= 0 };
}

describe("pointsInBrush", () => {
  it("keeps only points inside the events×tokens data window", () => {
    scenario(
      () => [pt("in", 50, 5000), pt("lowEv", 2, 5000), pt("hiTok", 50, 999999)],
      (pts) => pointsInBrush(pts, { ev0: 10, ev1: 100, tok0: 1000, tok1: 100000, includeZero: false }),
      (out) => {
        expect(out.map((p) => p.id)).toEqual(["in"]);
      },
    );
  });

  it("sorts the brushed subset by tokens descending (most-productive first)", () => {
    scenario(
      () => [pt("small", 20, 1000), pt("big", 30, 9000), pt("mid", 25, 4000)],
      (pts) => pointsInBrush(pts, { ev0: 1, ev1: 1000, tok0: 1, tok1: 1e9, includeZero: false }),
      (out) => {
        expect(out.map((p) => p.id)).toEqual(["big", "mid", "small"]);
      },
    );
  });

  it("excludes zero-token (uninstrumented) points unless includeZero is set", () => {
    scenario(
      () => [pt("real", 40, 3000), pt("zero", 40, 0)],
      (pts) => ({
        without: pointsInBrush(pts, { ev0: 1, ev1: 1000, tok0: 0, tok1: 1e9, includeZero: false }).map((p) => p.id),
        with: pointsInBrush(pts, { ev0: 1, ev1: 1000, tok0: 0, tok1: 1e9, includeZero: true }).map((p) => p.id),
      }),
      (r) => {
        expect(r.without).toEqual(["real"]);
        expect(r.with).toContain("zero");
        expect(r.with).toContain("real");
      },
    );
  });

  it("includes a zero point only when it also falls in the brushed events window", () => {
    scenario(
      () => [pt("zeroIn", 40, 0), pt("zeroOut", 5, 0)],
      (pts) => pointsInBrush(pts, { ev0: 10, ev1: 100, tok0: 0, tok1: 1e9, includeZero: true }).map((p) => p.id),
      (ids) => {
        expect(ids).toEqual(["zeroIn"]);
      },
    );
  });
});
