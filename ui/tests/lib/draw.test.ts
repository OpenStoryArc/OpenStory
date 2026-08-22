import { describe, it, expect } from "vitest";
import {
  applyDrawIntent,
  clientToUnitPoint,
  EMPTY_SCENE,
  flowerStrokes,
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

describe("clientToUnitPoint", () => {
  it("maps with stretch (preserveAspectRatio none)", () => {
    const rect = { left: 100, top: 50, width: 400, height: 200 };
    // center of element
    expect(clientToUnitPoint(300, 150, rect, { fit: "none" })).toEqual({ x: 0.5, y: 0.5 });
  });

  it("accounts for letterboxing when fit is meet (wide element)", () => {
    // viewBox 1000x1000 in a 400x200 rect → content is 200x200 centered, x offset 100
    const rect = { left: 0, top: 0, width: 400, height: 200 };
    // content left edge at x=100 in client space → unit 0
    expect(clientToUnitPoint(100, 100, rect, { fit: "meet" })).toMatchObject({ x: 0, y: 0.5 });
    // content center
    expect(clientToUnitPoint(200, 100, rect, { fit: "meet" })).toMatchObject({ x: 0.5, y: 0.5 });
    // content right edge at x=300
    expect(clientToUnitPoint(300, 100, rect, { fit: "meet" })).toMatchObject({ x: 1, y: 0.5 });
  });

  it("naive width mapping would be wrong under meet — document the bug we fixed", () => {
    const rect = { left: 0, top: 0, width: 400, height: 200 };
    const naiveX = 200 / 400; // 0.5 — looks like center of *element*
    const unit = clientToUnitPoint(200, 100, rect, { fit: "meet" });
    // under meet, client x=200 is content center (0.5) — coincidentally same;
    // client x=100 is content left (0), not 100/400=0.25
    expect(clientToUnitPoint(100, 100, rect, { fit: "meet" })!.x).toBe(0);
    expect(naiveX).toBe(0.5);
    expect(unit!.x).toBe(0.5);
    expect(clientToUnitPoint(100, 100, rect, { fit: "none" })!.x).toBe(0.25);
  });
});

describe("flowerStrokes", () => {
  it("returns a dense multi-layer botanical with filled paths", () => {
    const s = flowerStrokes();
    expect(s.length).toBeGreaterThan(30);
    const filledPaths = s.filter(
      (x) => x.type === "path" && x.fill && x.fill !== "none",
    );
    expect(filledPaths.length).toBeGreaterThan(8);
    expect(s.some((x) => x.type === "text")).toBe(true);
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

  it("hides without clearing strokes (navigate / read history)", () => {
    const a = applyDrawIntent(EMPTY_SCENE, {
      strokes: [{ type: "circle", cx: 0.5, cy: 0.5, r: 0.1 }],
      label: "dogfood",
    });
    const hidden = applyDrawIntent(a, { visible: false, strokes: [] });
    expect(hidden.visible).toBe(false);
    expect(hidden.strokes).toHaveLength(1);
    expect(hidden.label).toBe("dogfood");
    const shown = applyDrawIntent(hidden, { visible: true, strokes: [] });
    expect(shown.visible).toBe(true);
    expect(shown.strokes).toHaveLength(1);
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
