import { describe, it, expect } from "vitest";
import {
  chainEdges,
  contrastStretch,
  dualEdgeTrace,
  edgeTraceImageData,
  filterStrokesToEllipse,
  sobelMagnitude,
  stippleFromImageData,
  syntheticFaceImageData,
  toGray,
} from "@/lib/draw-trace";

describe("edge trace pure pipeline", () => {
  it("toGray preserves length", () => {
    const img = syntheticFaceImageData(32);
    expect(toGray(img).length).toBe(32 * 32);
  });

  it("sobel finds non-zero magnitude on a disk", () => {
    const img = syntheticFaceImageData(48);
    const g = toGray(img);
    const mag = sobelMagnitude(g, 48, 48);
    let max = 0;
    for (let i = 0; i < mag.length; i++) max = Math.max(max, mag[i]!);
    expect(max).toBeGreaterThan(50);
  });

  it("edgeTraceImageData emits path strokes (real ink, not an image tag)", () => {
    const img = syntheticFaceImageData(64);
    const strokes = edgeTraceImageData(img, { edgeThreshold: 60, maxPaths: 80, stride: 1 });
    expect(strokes.length).toBeGreaterThan(3);
    expect(strokes.every((s) => s.type === "path")).toBe(true);
    const pts = strokes.flatMap((s) => (s.type === "path" ? s.points : []));
    expect(pts.every((p) => p.x >= 0 && p.x <= 1 && p.y >= 0 && p.y <= 1)).toBe(true);
  });

  it("stippleFromImageData emits circles for dark samples", () => {
    const img = syntheticFaceImageData(48);
    const dots = stippleFromImageData(img, { step: 3, maxDots: 200, darkBelow: 100 });
    expect(dots.length).toBeGreaterThan(5);
    expect(dots.every((s) => s.type === "circle")).toBe(true);
  });

  it("chainEdges is deterministic", () => {
    const img = syntheticFaceImageData(40);
    const g = toGray(img);
    const mag = sobelMagnitude(g, 40, 40);
    const a = chainEdges(mag, 40, 40, 50, 1, 40, 4);
    const b = chainEdges(mag, 40, 40, 50, 1, 40, 4);
    expect(a).toEqual(b);
  });

  it("contrastStretch expands dynamic range", () => {
    const g = new Float32Array([50, 100, 150]);
    const c = contrastStretch(g, 1.2);
    expect(c[0]).toBeLessThan(c[2]!);
    expect(Math.min(...c)).toBeGreaterThanOrEqual(0);
    expect(Math.max(...c)).toBeLessThanOrEqual(255);
  });

  it("dualEdgeTrace returns more ink than a sparse single pass", () => {
    const img = syntheticFaceImageData(64);
    const dual = dualEdgeTrace(img, { edgeThreshold: 100, softThreshold: 50, maxPaths: 40 });
    expect(dual.length).toBeGreaterThan(3);
    expect(dual.every((s) => s.type === "path")).toBe(true);
  });

  it("filterStrokesToEllipse drops far-away paths", () => {
    const kept = filterStrokesToEllipse(
      [
        { type: "path", points: [{ x: 0.5, y: 0.5 }, { x: 0.51, y: 0.51 }], stroke: "#fff" },
        { type: "path", points: [{ x: 0.05, y: 0.05 }, { x: 0.06, y: 0.06 }], stroke: "#fff" },
      ],
      0.5,
      0.5,
      0.2,
      0.2,
    );
    expect(kept).toHaveLength(1);
  });
});
