/**
 * Spec: personColor — deterministic hue per user identifier.
 *
 * Pure function; same input → same output. No randomness, no time
 * dependence, no I/O. The contract is what the UI relies on:
 * one user's chip / badge / label always shares a color.
 */

import { describe, it, expect } from "vitest";
import { personColor } from "@/lib/person-color";

describe("personColor", () => {
  it("returns the same color for the same user", () => {
    const a = personColor("katie");
    const b = personColor("katie");
    expect(a).toBe(b);
  });

  it("returns different colors for different users (high probability)", () => {
    // The palette is 10 colors; with 5 distinct names hash collisions
    // are possible but rare. Assert the set has at least 2 distinct
    // colors — that's enough to guarantee the function isn't a constant.
    const palette = new Set(
      ["katie", "max", "alex", "sam", "jordan"].map(personColor),
    );
    expect(palette.size).toBeGreaterThan(1);
  });

  it("returns a value from the theme-aware identity slots", () => {
    // Colors are now CSS custom-property references (--sc-0..9 in
    // index.css) rather than literal hexes, so the same palette renders
    // correctly in both light and dark themes.
    expect(personColor("katie")).toMatch(/^var\(--sc-[0-9]\)$/);
  });

  it("handles empty string without throwing", () => {
    // hash defaults to 0; idx = 0; index 0 is the first color.
    expect(() => personColor("")).not.toThrow();
    expect(personColor("")).toBe("var(--sc-0)");
  });
});
