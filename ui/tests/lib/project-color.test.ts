/**
 * Spec: projectColor — deterministic hue per project identifier.
 *
 * Mirror of person-color.test.ts. Different palette, same contract:
 * pure function, same input → same output, hex shape, empty-safe.
 */

import { describe, it, expect } from "vitest";
import { projectColor } from "@/lib/project-color";

describe("projectColor", () => {
  it("returns the same color for the same project", () => {
    expect(projectColor("OpenStory")).toBe(projectColor("OpenStory"));
  });

  it("returns a hex color from the palette", () => {
    expect(projectColor("OpenStory")).toMatch(/^#[0-9a-f]{6}$/i);
  });

  it("returns at least 2 distinct colors across a small set", () => {
    const set = new Set(
      ["OpenStory", "openstory-ui-prototype", "agentic-learning"].map(projectColor),
    );
    expect(set.size).toBeGreaterThan(1);
  });

  it("handles empty string without throwing", () => {
    expect(() => projectColor("")).not.toThrow();
    expect(projectColor("")).toMatch(/^#[0-9a-f]{6}$/i);
  });
});
