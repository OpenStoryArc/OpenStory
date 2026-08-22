import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  marginaliaPathStrokes,
  penSceneToWire,
  strokeBounds,
  strokeKindHistogram,
  summarizePenWire,
} from "@/lib/pen-eyes";
import type { DrawScene } from "@/lib/draw";

describe("strokeBounds", () => {
  it("bounds a path and circle", () => {
    scenario(
      () =>
        strokeBounds([
          {
            type: "path",
            points: [
              { x: 0.1, y: 0.2 },
              { x: 0.5, y: 0.6 },
            ],
          },
          { type: "circle", cx: 0.8, cy: 0.3, r: 0.1 },
        ]),
      (b) => b,
      (b) => {
        expect(b!.x).toBeCloseTo(0.1);
        expect(b!.y).toBeCloseTo(0.2);
        expect(b!.w).toBeCloseTo(0.8); // 0.9 - 0.1
        expect(b!.h).toBeCloseTo(0.4); // 0.6 - 0.2
      },
    );
  });

  it("returns null for empty", () => {
    expect(strokeBounds([])).toBeNull();
  });
});

describe("penSceneToWire", () => {
  it("marks empty scenes", () => {
    const w = penSceneToWire(
      { strokes: [], visible: true },
      { now: () => "t0" },
    );
    expect(w.empty).toBe(true);
    expect(w.stroke_count).toBe(0);
    expect(w.at).toBe("t0");
    expect(summarizePenWire(w)).toBe("pen empty");
  });

  it("truncates stroke list and downsamples long paths", () => {
    const longPath = {
      type: "path" as const,
      points: Array.from({ length: 200 }, (_, i) => ({
        x: i / 200,
        y: 0.5,
      })),
      stroke: "#000",
    };
    const strokes = Array.from({ length: 50 }, () => longPath);
    const scene: DrawScene = { strokes, visible: true, label: "arch" };
    const w = penSceneToWire(scene, {
      maxStrokes: 10,
      maxPointsPerPath: 8,
      now: () => "t1",
    });
    expect(w.stroke_count).toBe(50);
    expect(w.strokes).toHaveLength(10);
    expect(w.truncated).toBe(true);
    expect(w.label).toBe("arch");
    if (w.strokes[0]?.type === "path") {
      expect(w.strokes[0].points.length).toBeLessThanOrEqual(8);
    }
    expect(summarizePenWire(w)).toContain("50 stroke");
    expect(summarizePenWire(w)).toContain("truncated");
  });

  it("histograms kinds", () => {
    expect(
      strokeKindHistogram([
        { type: "line", x1: 0, y1: 0, x2: 1, y2: 1 },
        { type: "line", x1: 0, y1: 1, x2: 1, y2: 0 },
        { type: "text", x: 0.5, y: 0.5, text: "NATS" },
      ]),
    ).toEqual({ line: 2, text: 1 });
  });

  it("carries interactive/annotate flag for journey eyes", () => {
    const w = penSceneToWire(
      { strokes: [], visible: true },
      { interactive: true, now: () => "t" },
    );
    expect(w.interactive).toBe(true);
    expect(w.empty).toBe(true);
    expect(summarizePenWire(w)).toContain("annotating");
  });

  it("marginaliaPathStrokes is a high-contrast pair", () => {
    const s = marginaliaPathStrokes([
      { x: 0.1, y: 0.1 },
      { x: 0.5, y: 0.5 },
    ]);
    expect(s).toHaveLength(2);
    expect(s[0]).toMatchObject({ type: "path", stroke: "#0f172a" });
    expect(s[1]).toMatchObject({ type: "path", stroke: "#facc15" });
  });
});
