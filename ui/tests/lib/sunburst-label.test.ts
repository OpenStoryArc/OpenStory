import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { sunburstLabelLayout } from "@/lib/sunburst-label";

describe("sunburstLabelLayout", () => {
  it("shows a label on a big, thick wedge", () => {
    scenario(
      // ~quarter turn, thick ring at good radius
      () => sunburstLabelLayout({ x0: 0, x1: Math.PI / 2, y0: 60, y1: 120 }),
      (l) => l.show,
      (show) => expect(show).toBe(true),
    );
  });

  it("hides the label on a thin angular sliver", () => {
    scenario(
      () => sunburstLabelLayout({ x0: 0, x1: 0.01, y0: 60, y1: 120 }),
      (l) => l.show,
      (show) => expect(show).toBe(false),
    );
  });

  it("hides the label on a radially-thin ring (no room for text length)", () => {
    scenario(
      () => sunburstLabelLayout({ x0: 0, x1: Math.PI, y0: 100, y1: 110 }),
      (l) => l.show,
      (show) => expect(show).toBe(false),
    );
  });

  it("flips text on the left half (mid-angle past π) to stay upright", () => {
    scenario(
      () => ({
        right: sunburstLabelLayout({ x0: 0, x1: Math.PI / 2, y0: 60, y1: 120 }),
        left: sunburstLabelLayout({ x0: Math.PI, x1: 1.5 * Math.PI, y0: 60, y1: 120 }),
      }),
      (r) => r,
      (r) => {
        expect(r.right.flip).toBe(false);
        expect(r.left.flip).toBe(true);
      },
    );
  });

  it("scales maxChars with ring thickness", () => {
    scenario(
      () => ({
        thin: sunburstLabelLayout({ x0: 0, x1: Math.PI / 2, y0: 60, y1: 90 }),
        thick: sunburstLabelLayout({ x0: 0, x1: Math.PI / 2, y0: 60, y1: 160 }),
      }),
      (r) => r,
      (r) => expect(r.thick.maxChars).toBeGreaterThan(r.thin.maxChars),
    );
  });
});
