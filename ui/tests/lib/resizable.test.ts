import { describe, it, expect } from "vitest";
import { clampWidth } from "@/lib/resizable";

/** The pure core of the resizable side panel: a width is always kept within
 *  [min, max] so a drag can never collapse the panel or push it off-screen. */
describe("clampWidth — keep a dragged panel width in bounds", () => {
  it("returns the width unchanged when within bounds", () => {
    expect(clampWidth(500, 320, 760)).toBe(500);
  });

  it("clamps below the minimum up to min", () => {
    expect(clampWidth(100, 320, 760)).toBe(320);
  });

  it("clamps above the maximum down to max", () => {
    expect(clampWidth(9000, 320, 760)).toBe(760);
  });

  it("returns the bound exactly at the edges", () => {
    expect(clampWidth(320, 320, 760)).toBe(320);
    expect(clampWidth(760, 320, 760)).toBe(760);
  });

  it("coerces a non-finite width to the minimum (drag glitch guard)", () => {
    expect(clampWidth(NaN, 320, 760)).toBe(320);
    expect(clampWidth(Infinity, 320, 760)).toBe(760);
  });
});
