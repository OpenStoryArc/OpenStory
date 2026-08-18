import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  diagramLabelsToStrokes,
  normalizeStopKind,
  stopRequiresEventAnchor,
} from "@/lib/reel-visual";

describe("normalizeStopKind", () => {
  it("defaults unknown to spotlight", () => {
    expect(normalizeStopKind(undefined)).toBe("spotlight");
    expect(normalizeStopKind("diagram")).toBe("diagram");
  });
});

describe("stopRequiresEventAnchor", () => {
  it("only spotlight requires history anchor", () => {
    expect(stopRequiresEventAnchor("spotlight")).toBe(true);
    expect(stopRequiresEventAnchor("diagram")).toBe(false);
    expect(stopRequiresEventAnchor("title")).toBe(false);
    expect(stopRequiresEventAnchor("image")).toBe(false);
  });
});

describe("diagramLabelsToStrokes", () => {
  it("emits boxes and text for each label", () => {
    scenario(
      () => diagramLabelsToStrokes(["Bash ×3", "Edit · auth.rs"], { title: "Journey" }),
      (s) => s,
      (s) => {
        expect(s.some((x) => x.type === "text" && x.text === "Journey")).toBe(true);
        expect(s.filter((x) => x.type === "path" && x.closed).length).toBe(2);
        expect(s.some((x) => x.type === "text" && String(x.text).includes("Bash"))).toBe(true);
      },
    );
  });

  it("handles empty labels", () => {
    const s = diagramLabelsToStrokes([]);
    expect(s.some((x) => x.type === "text")).toBe(true);
  });

  describe("diagramLabelsToStrokes palette", () => {
    it("draws every box in the accent ink — color rotation encodes nothing and is gone", () => {
      const strokes = diagramLabelsToStrokes(["ToolSearch", "Bash ×3", "Edit"], { title: "Journey" });
      const boxes = strokes.filter((s) => s.type === "path");
      expect(boxes.length).toBe(3);
      for (const b of boxes) expect(b.stroke).toBe("#7aa2f7");
    });

    it("fits box width to the label instead of one wide bar", () => {
      const strokes = diagramLabelsToStrokes(["Bash", "a-much-longer-tool-label"]);
      const [short, long] = strokes.filter((s) => s.type === "path");
      const width = (p: { points: readonly { x: number }[] }) =>
        Math.max(...p.points.map((q) => q.x)) - Math.min(...p.points.map((q) => q.x));
      expect(width(short as never)).toBeLessThan(width(long as never));
    });
  });
});
