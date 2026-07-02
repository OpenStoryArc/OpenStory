import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { sunburstLabelLayout, sunburstCenterText } from "@/lib/sunburst-label";

describe("sunburstCenterText", () => {
  it("shows the hovered wedge's name + metric value + a click hint", () => {
    scenario(
      () => sunburstCenterText({ name: "OpenStory", value: 3480, kind: "project" }, { name: "all", depth: 0 }, "events"),
      (c) => c,
      (c) => { expect(c.primary).toBe("OpenStory"); expect(c.secondary).toContain("3,480 events"); expect(c.secondary).toContain("zoom in"); },
    );
  });

  it("teaches the interaction — a session says 'open', a group says 'zoom in'", () => {
    scenario(
      () => ({
        session: sunburstCenterText({ name: "s", value: 1, kind: "session" }, { name: "all", depth: 0 }, "events"),
        group: sunburstCenterText({ name: "g", value: 1, kind: "project" }, { name: "all", depth: 0 }, "events"),
      }),
      (r) => r,
      ({ session, group }) => {
        expect(session.secondary).toContain("open");
        expect(group.secondary).toContain("zoom");
      },
    );
  });

  it("falls back to the focus name (or 'all' at the root) when nothing is hovered", () => {
    scenario(
      () => ({ root: sunburstCenterText(null, { name: "x", depth: 0 }, "events"),
               deep: sunburstCenterText(null, { name: "workspace", depth: 2 }, "events") }),
      (r) => r,
      ({ root, deep }) => { expect(root.primary).toBe("all"); expect(deep.primary).toBe("workspace"); },
    );
  });

  it("truncates a long hovered name to fit the center hole", () => {
    scenario(
      () => sunburstCenterText({ name: "a-really-long-session-title-that-wont-fit", value: 1 }, { name: "all", depth: 0 }, "events"),
      (c) => c.primary.length,
      (len) => expect(len).toBeLessThanOrEqual(18),
    );
  });
});

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
