import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { sortByScore, type VizCandidate } from "@/lib/viz-candidates";

function cand(id: string, score: number, over: Partial<VizCandidate> = {}): VizCandidate {
  return { id, name: id, d3_shape: "x", data_shape: "y", what_it_shows: "", openstory_fields_used: [], novelty: 1, insight_value: 1, build_cost: 1, score, hypothesis: "h", falsifier: "f", witness: "w", ...over };
}

describe("sortByScore", () => {
  it("orders candidates by score descending", () => {
    scenario(
      () => sortByScore([cand("a", 5.3), cand("b", 9.0), cand("c", 8.0)]),
      (out) => out.map((c) => c.id),
      (ids) => expect(ids).toEqual(["b", "c", "a"]),
    );
  });

  it("breaks ties by name and does not mutate the input", () => {
    scenario(
      () => { const input = [cand("zebra", 8), cand("alpha", 8)]; const out = sortByScore(input); return { out: out.map((c) => c.id), inputFirst: input[0]!.id }; },
      (r) => r,
      (r) => {
        expect(r.out).toEqual(["alpha", "zebra"]);
        expect(r.inputFirst).toBe("zebra"); // original untouched
      },
    );
  });
});
