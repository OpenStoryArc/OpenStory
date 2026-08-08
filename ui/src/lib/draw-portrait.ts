/**
 * Async portrait recipes for the agent pen — fetch image → ink strokes.
 * Never writes history; only produces DrawStroke[] for ui.* ink.
 */

import type { DrawStroke } from "@/lib/draw";
import {
  dualEdgeTrace,
  filterStrokesToEllipse,
  stippleFromImageData,
  type PixelBuffer,
  type TraceRect,
} from "@/lib/draw-trace";

/** Tighter portrait frame — more face, less wall. */
const MAX_FACE: TraceRect = { x: 0.3, y: 0.05, w: 0.4, h: 0.48 };

/** Load URL into pixel buffer (browser). Uses crossOrigin anonymous for canvas read. */
export async function loadImageData(
  href: string,
  maxSide = 200,
): Promise<PixelBuffer> {
  const img = new Image();
  img.crossOrigin = "anonymous";
  await new Promise<void>((resolve, reject) => {
    img.onload = () => resolve();
    img.onerror = () => reject(new Error(`failed to load image: ${href.slice(0, 80)}`));
    img.src = href;
  });
  const scale = Math.min(1, maxSide / Math.max(img.naturalWidth, img.naturalHeight, 1));
  const w = Math.max(8, Math.round(img.naturalWidth * scale));
  const h = Math.max(8, Math.round(img.naturalHeight * scale));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("2d context unavailable");
  // Slight upscale sharpen: draw then mild contrast is in edge pipeline
  ctx.drawImage(img, 0, 0, w, h);
  const id = ctx.getImageData(0, 0, w, h);
  return { width: id.width, height: id.height, data: id.data };
}

/**
 * Handsomer ink portrait: dual edges + soft stipple, ellipse-masked to face.
 * No type:image paste — only path/circle/text/ellipse ink.
 */
export async function portraitInkStrokes(
  href: string,
  opts?: {
    readonly label?: string;
    readonly caption?: string;
    readonly rect?: TraceRect;
  },
): Promise<DrawStroke[]> {
  const rect = opts?.rect ?? MAX_FACE;
  const data = await loadImageData(href, 200);
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  const rx = rect.w * 0.46;
  const ry = rect.h * 0.48;

  const edges = dualEdgeTrace(data, {
    rect,
    edgeThreshold: 105,
    softThreshold: 52,
    maxPaths: 80,
    minPathLen: 7,
    stride: 1,
    color: "#e7e5e4",
    strokeWidth: 2.1,
    softWidth: 0.85,
  });

  // Form shadow: sparse, soft dots only (less brick noise)
  const stipple = stippleFromImageData(data, {
    rect,
    step: 4,
    maxDots: 420,
    darkBelow: 95,
    color: "#78716c",
  });

  const ink = filterStrokesToEllipse([...stipple, ...edges], cx, cy, rx, ry);

  const caption = opts?.caption ?? opts?.label ?? "portrait";
  const strokes: DrawStroke[] = [
    // soft vignette ring (handsome frame)
    {
      type: "ellipse",
      cx,
      cy,
      rx: rx * 1.02,
      ry: ry * 1.02,
      stroke: "#44403c",
      strokeWidth: 6,
      fill: "none",
    },
    {
      type: "ellipse",
      cx,
      cy,
      rx,
      ry,
      stroke: "#a8a29e",
      strokeWidth: 1.5,
      fill: "none",
    },
    ...ink,
    {
      type: "text",
      x: 0.5,
      y: Math.min(0.92, rect.y + rect.h + 0.07),
      text: caption,
      fill: "#fafaf9",
      fontSize: 26,
    },
    {
      type: "text",
      x: 0.5,
      y: Math.min(0.96, rect.y + rect.h + 0.12),
      text: "dual-edge + stipple ink · handsome mode v0.2",
      fill: "#a8a29e",
      fontSize: 11,
    },
  ];
  return strokes;
}

/** Geometric “Max” caricature — pure vectors, no network (fallback). */
export function geometricMaxStrokes(): DrawStroke[] {
  const color = "#e7e5e4";
  const cx = 0.5;
  const cy = 0.32;
  return [
    { type: "ellipse", cx, cy, rx: 0.14, ry: 0.17, stroke: color, strokeWidth: 2.5, fill: "none" },
    // hair
    {
      type: "path",
      stroke: color,
      strokeWidth: 2.2,
      points: [
        { x: 0.38, y: 0.28 },
        { x: 0.4, y: 0.17 },
        { x: 0.5, y: 0.13 },
        { x: 0.6, y: 0.17 },
        { x: 0.62, y: 0.28 },
      ],
    },
    // eyes — slightly smiling
    { type: "ellipse", cx: 0.44, cy: 0.3, rx: 0.028, ry: 0.014, fill: color, stroke: color },
    { type: "ellipse", cx: 0.56, cy: 0.3, rx: 0.028, ry: 0.014, fill: color, stroke: color },
    // brows
    { type: "line", x1: 0.4, y1: 0.255, x2: 0.47, y2: 0.25, stroke: color, strokeWidth: 2 },
    { type: "line", x1: 0.53, y1: 0.25, x2: 0.6, y2: 0.255, stroke: color, strokeWidth: 2 },
    // nose
    {
      type: "path",
      stroke: color,
      strokeWidth: 1.5,
      points: [
        { x: 0.5, y: 0.3 },
        { x: 0.49, y: 0.355 },
        { x: 0.52, y: 0.365 },
      ],
    },
    // smile — a bit wider (handsome mode)
    {
      type: "path",
      stroke: color,
      strokeWidth: 2.2,
      points: [
        { x: 0.42, y: 0.395 },
        { x: 0.46, y: 0.43 },
        { x: 0.5, y: 0.445 },
        { x: 0.54, y: 0.43 },
        { x: 0.58, y: 0.395 },
      ],
    },
    // collar / suit
    {
      type: "path",
      stroke: color,
      strokeWidth: 2,
      points: [
        { x: 0.39, y: 0.47 },
        { x: 0.45, y: 0.52 },
        { x: 0.5, y: 0.5 },
        { x: 0.55, y: 0.52 },
        { x: 0.61, y: 0.47 },
      ],
    },
    { type: "text", x: 0.5, y: 0.62, text: "Max Glassie", fill: color, fontSize: 26 },
    {
      type: "text",
      x: 0.5,
      y: 0.68,
      text: "geometric ink · handsome mode · ui.* only",
      fill: "#a8a29e",
      fontSize: 12,
    },
  ];
}
