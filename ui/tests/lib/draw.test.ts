import { describe, it, expect } from "vitest";
import {
  applyDrawIntent,
  EMPTY_SCENE,
  normalizeStroke,
  normalizeStrokes,
  pathToSvgD,
  smileyStrokes,
} from "@/lib/draw";

describe("normalizeStroke", () => {
  it("accepts a circle in unit space", () => {
    const s = normalizeStroke({ type: "circle", cx: 0.5, cy: 0.5, r: 0.1, stroke: "#000" });
    expect(s).toEqual({
      type: "circle",
      cx: 0.5,
      cy: 0.5,
      r: 0.1,
      stroke: "#000",
      strokeWidth: undefined,
      fill: undefined,
    });
  });

  it("rejects javascript: image hrefs", () => {
    expect(
      normalizeStroke({
        type: "image",
        href: "javascript:alert(1)",
        x: 0,
        y: 0,
        w: 0.2,
        h: 0.2,
      }),
    ).toBeNull();
  });

  it("accepts https image", () => {
    const s = normalizeStroke({
      type: "image",
      href: "https://github.com/maxglassie.png",
      x: 0.3,
      y: 0.2,
      w: 0.3,
      h: 0.3,
    });
    expect(s?.type).toBe("image");
    if (s?.type === "image") expect(s.href).toContain("maxglassie");
  });

  it("clamps coordinates into 0..1", () => {
    const s = normalizeStroke({ type: "line", x1: -1, y1: 2, x2: 0.5, y2: 0.5 });
    expect(s).toMatchObject({ type: "line", x1: 0, y1: 1, x2: 0.5, y2: 0.5 });
  });
});

describe("applyDrawIntent", () => {
  it("appends by default", () => {
    const a = applyDrawIntent(EMPTY_SCENE, {
      strokes: [{ type: "circle", cx: 0.5, cy: 0.5, r: 0.1 }],
    });
    const b = applyDrawIntent(a, {
      strokes: [{ type: "line", x1: 0, y1: 0, x2: 1, y2: 1 }],
    });
    expect(b.strokes).toHaveLength(2);
  });

  it("clears then draws", () => {
    const a = applyDrawIntent(EMPTY_SCENE, {
      strokes: [{ type: "circle", cx: 0.5, cy: 0.5, r: 0.1 }],
    });
    const b = applyDrawIntent(a, {
      clear: true,
      strokes: [{ type: "text", x: 0.5, y: 0.5, text: "hi" }],
    });
    expect(b.strokes).toHaveLength(1);
    expect(b.strokes[0]).toMatchObject({ type: "text", text: "hi" });
  });
});

describe("smileyStrokes", () => {
  it("returns face + eyes + smile", () => {
    const s = smileyStrokes();
    expect(s.some((x) => x.type === "circle")).toBe(true);
    expect(s.some((x) => x.type === "path")).toBe(true);
    expect(normalizeStrokes(s)).toHaveLength(s.length);
  });
});

describe("pathToSvgD", () => {
  it("builds M/L commands in 1000-space", () => {
    expect(pathToSvgD([{ x: 0, y: 0 }, { x: 1, y: 1 }])).toBe("M 0.0 0.0 L 1000.0 1000.0");
  });
});
