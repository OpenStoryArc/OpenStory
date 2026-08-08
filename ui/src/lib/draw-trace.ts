/**
 * Raster → vector ink: edge detection + stipple for the agent pen.
 * Pure over ImageData; loaders live at the edge (browser fetch).
 * Output is DrawStroke paths/circles in NORMALIZED 0..1 space within a target rect.
 */

import type { DrawStroke, NormPoint } from "@/lib/draw";

/** Minimal image buffer (ImageData-compatible) so tests run without DOM ImageData. */
export interface PixelBuffer {
  readonly width: number;
  readonly height: number;
  readonly data: Uint8ClampedArray;
}

export interface TraceRect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

export interface TraceOptions {
  /** Sobel magnitude threshold 0..~1442; default ~80. */
  readonly edgeThreshold?: number;
  /** Max path segments (cap for UI perf). */
  readonly maxPaths?: number;
  /** Min points per path. */
  readonly minPathLen?: number;
  /** Stride when scanning (2 = every other pixel). */
  readonly stride?: number;
  readonly color?: string;
  readonly strokeWidth?: number;
  /** Target placement in viewport 0..1. */
  readonly rect?: TraceRect;
}

const DEFAULT_RECT: TraceRect = { x: 0.28, y: 0.08, w: 0.44, h: 0.52 };

function luminance(r: number, g: number, b: number): number {
  return 0.299 * r + 0.587 * g + 0.114 * b;
}

/** Grayscale float buffer [0,255] length w*h. */
export function toGray(data: PixelBuffer): Float32Array {
  const { width: w, height: h, data: px } = data;
  const out = new Float32Array(w * h);
  for (let i = 0, p = 0; i < px.length; i += 4, p++) {
    out[p] = luminance(px[i]!, px[i + 1]!, px[i + 2]!);
  }
  return out;
}

/** Sobel magnitude per pixel. Border = 0. */
export function sobelMagnitude(gray: Float32Array, w: number, h: number): Float32Array {
  const mag = new Float32Array(w * h);
  for (let y = 1; y < h - 1; y++) {
    for (let x = 1; x < w - 1; x++) {
      const i = y * w + x;
      const gx =
        -gray[i - w - 1]! +
        gray[i - w + 1]! +
        -2 * gray[i - 1]! +
        2 * gray[i + 1]! +
        -gray[i + w - 1]! +
        gray[i + w + 1]!;
      const gy =
        -gray[i - w - 1]! -
        2 * gray[i - w]! -
        gray[i - w + 1]! +
        gray[i + w - 1]! +
        2 * gray[i + w]! +
        gray[i + w + 1]!;
      mag[i] = Math.hypot(gx, gy);
    }
  }
  return mag;
}

/**
 * Greedy chain of edge pixels into polylines (image pixel space).
 * Pure, deterministic for a fixed mag buffer.
 */
export function chainEdges(
  mag: Float32Array,
  w: number,
  h: number,
  threshold: number,
  stride: number,
  maxPaths: number,
  minPathLen: number,
): number[][] {
  const seen = new Uint8Array(w * h);
  const paths: number[][] = [];

  const isEdge = (x: number, y: number): boolean => {
    if (x < 1 || y < 1 || x >= w - 1 || y >= h - 1) return false;
    const i = y * w + x;
    return mag[i]! >= threshold && seen[i] === 0;
  };

  const neighbors = [
    [1, 0],
    [-1, 0],
    [0, 1],
    [0, -1],
    [1, 1],
    [1, -1],
    [-1, 1],
    [-1, -1],
  ];

  for (let y = 1; y < h - 1 && paths.length < maxPaths; y += stride) {
    for (let x = 1; x < w - 1 && paths.length < maxPaths; x += stride) {
      if (!isEdge(x, y)) continue;
      const path: number[] = [];
      let cx = x;
      let cy = y;
      while (path.length < 400) {
        const i = cy * w + cx;
        if (seen[i]) break;
        seen[i] = 1;
        path.push(cx, cy);
        let found = false;
        for (const [dx, dy] of neighbors) {
          const nx = cx + dx!;
          const ny = cy + dy!;
          if (isEdge(nx, ny)) {
            cx = nx;
            cy = ny;
            found = true;
            break;
          }
        }
        if (!found) break;
      }
      if (path.length / 2 >= minPathLen) paths.push(path);
    }
  }
  return paths;
}

/** Pixel paths → normalized DrawStroke paths inside rect. */
export function pathsToStrokes(
  paths: readonly number[][],
  imgW: number,
  imgH: number,
  rect: TraceRect,
  color: string,
  strokeWidth: number,
): DrawStroke[] {
  const strokes: DrawStroke[] = [];
  for (const path of paths) {
    const points: NormPoint[] = [];
    for (let i = 0; i < path.length; i += 2) {
      const px = path[i]!;
      const py = path[i + 1]!;
      points.push({
        x: rect.x + (px / Math.max(imgW - 1, 1)) * rect.w,
        y: rect.y + (py / Math.max(imgH - 1, 1)) * rect.h,
      });
    }
    if (points.length >= 2) {
      strokes.push({
        type: "path",
        points,
        stroke: color,
        strokeWidth,
        fill: "none",
      });
    }
  }
  return strokes;
}

/**
 * Stipple: dark samples → small filled circles (cross-hatch feel without a shader).
 * Pure over ImageData.
 */
export function stippleFromImageData(
  data: PixelBuffer,
  opts?: {
    readonly rect?: TraceRect;
    readonly step?: number;
    readonly maxDots?: number;
    readonly darkBelow?: number;
    readonly color?: string;
  },
): DrawStroke[] {
  const rect = opts?.rect ?? DEFAULT_RECT;
  const step = opts?.step ?? 4;
  const maxDots = opts?.maxDots ?? 800;
  const darkBelow = opts?.darkBelow ?? 140;
  const color = opts?.color ?? "#2f4a3e";
  const { width: w, height: h, data: px } = data;
  const strokes: DrawStroke[] = [];
  for (let y = 0; y < h && strokes.length < maxDots; y += step) {
    for (let x = 0; x < w && strokes.length < maxDots; x += step) {
      const i = (y * w + x) * 4;
      const L = luminance(px[i]!, px[i + 1]!, px[i + 2]!);
      if (L > darkBelow) continue;
      // denser / larger dots when darker
      const t = 1 - L / 255;
      const r = 0.0015 + t * 0.004;
      strokes.push({
        type: "circle",
        cx: rect.x + (x / Math.max(w - 1, 1)) * rect.w,
        cy: rect.y + (y / Math.max(h - 1, 1)) * rect.h,
        r,
        fill: color,
        stroke: color,
        strokeWidth: 0.5,
      });
    }
  }
  return strokes;
}

/** Full edge-trace pipeline from pixel buffer → path strokes. */
export function edgeTraceImageData(data: PixelBuffer, opts?: TraceOptions): DrawStroke[] {
  const threshold = opts?.edgeThreshold ?? 90;
  const maxPaths = opts?.maxPaths ?? 120;
  const minPathLen = opts?.minPathLen ?? 6;
  const stride = opts?.stride ?? 2;
  const color = opts?.color ?? "#2f4a3e";
  const strokeWidth = opts?.strokeWidth ?? 1.6;
  const rect = opts?.rect ?? DEFAULT_RECT;
  const gray = toGray(data);
  const mag = sobelMagnitude(gray, data.width, data.height);
  const paths = chainEdges(mag, data.width, data.height, threshold, stride, maxPaths, minPathLen);
  return pathsToStrokes(paths, data.width, data.height, rect, color, strokeWidth);
}

/**
 * Build a pure synthetic face buffer for tests: dark bg + bright disk + eyes.
 * Works in Node (no DOM) and browser.
 */
export function syntheticFaceImageData(size = 64): PixelBuffer {
  const w = size;
  const h = size;
  const buf = new Uint8ClampedArray(w * h * 4);
  const cx = w / 2;
  const cy = h / 2;
  const r = size * 0.32;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const d = Math.hypot(x - cx, y - cy);
      const on = d < r ? 220 : 30;
      // eye holes
      const leftEye = Math.hypot(x - (cx - r * 0.35), y - (cy - r * 0.2)) < r * 0.12;
      const rightEye = Math.hypot(x - (cx + r * 0.35), y - (cy - r * 0.2)) < r * 0.12;
      const v = leftEye || rightEye ? 20 : on;
      buf[i] = v;
      buf[i + 1] = v;
      buf[i + 2] = v;
      buf[i + 3] = 255;
    }
  }
  return { width: w, height: h, data: buf };
}
