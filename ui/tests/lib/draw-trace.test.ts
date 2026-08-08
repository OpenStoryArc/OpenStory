import { describe, it, expect } from "vitest";
import {
  chainEdges,
  edgeTraceImageData,
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
});
