import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { beeswarmOffsets } from "@/lib/beeswarm";

describe("beeswarmOffsets", () => {
  it("places a lone dot at y=0", () => {
    scenario(
      () => beeswarmOffsets([100], 4),
      (ys) => ys,
      (ys) => expect(ys).toEqual([0]),
    );
  });

  it("separates dots at the same x by at least a diameter", () => {
    scenario(
      () => beeswarmOffsets([50, 50, 50], 4),
      (ys) => ys,
      (ys) => {
        // all at same x → must be ≥ 2r apart in y
        const sorted = [...ys].sort((a, b) => a - b);
        for (let i = 1; i < sorted.length; i++) expect(sorted[i]! - sorted[i - 1]!).toBeGreaterThanOrEqual(8 - 1e-6);
        expect(Math.min(...ys.map(Math.abs))).toBe(0); // one sits at 0
      },
    );
  });

  it("produces no overlaps across a cluster", () => {
    scenario(
      () => { const xs = [10, 11, 12, 12, 12, 13, 40, 41]; return { xs, ys: beeswarmOffsets(xs, 3) }; },
      (r) => r,
      ({ xs, ys }) => {
        for (let i = 0; i < xs.length; i++)
          for (let j = i + 1; j < xs.length; j++) {
            const dx = xs[i]! - xs[j]!, dy = ys[i]! - ys[j]!;
            expect(Math.sqrt(dx * dx + dy * dy)).toBeGreaterThanOrEqual(6 - 1e-6);
          }
      },
    );
  });

  it("lets far-apart dots both sit at y=0", () => {
    scenario(
      () => beeswarmOffsets([0, 1000], 4),
      (ys) => ys,
      (ys) => expect(ys).toEqual([0, 0]),
    );
  });
});
