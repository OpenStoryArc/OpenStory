/**
 * Async portrait recipes for the agent pen — fetch image → ink strokes.
 * Never writes history; only produces DrawStroke[] for ui.* ink.
 */

import type { DrawStroke } from "@/lib/draw";
import {
  edgeTraceImageData,
  stippleFromImageData,
  type PixelBuffer,
  type TraceRect,
} from "@/lib/draw-trace";

const MAX_FACE: TraceRect = { x: 0.28, y: 0.06, w: 0.44, h: 0.5 };

/** Load URL into pixel buffer (browser). Uses crossOrigin anonymous for canvas read. */
export async function loadImageData(
  href: string,
  maxSide = 160,
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
  ctx.drawImage(img, 0, 0, w, h);
  const id = ctx.getImageData(0, 0, w, h);
  return { width: id.width, height: id.height, data: id.data };
}

/**
 * Really draw a portrait: edge paths + light stipple + caption.
 * No type:image paste — only path/circle/text ink.
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
  const data = await loadImageData(href, 140);
  const edges = edgeTraceImageData(data, {
    rect,
    edgeThreshold: 75,
    maxPaths: 100,
    minPathLen: 5,
    stride: 1,
    color: "#d6d3d1",
    strokeWidth: 1.4,
  });
  const stipple = stippleFromImageData(data, {
    rect,
    step: 3,
    maxDots: 600,
    darkBelow: 130,
    color: "#a8a29e",
  });
  const caption = opts?.caption ?? opts?.label ?? "portrait";
  const strokes: DrawStroke[] = [
    // faint frame (vector)
    {
      type: "ellipse",
      cx: rect.x + rect.w / 2,
      cy: rect.y + rect.h / 2,
      rx: rect.w * 0.48,
      ry: rect.h * 0.48,
      stroke: "#57534e",
      strokeWidth: 2,
      fill: "none",
    },
    ...stipple,
    ...edges,
    {
      type: "text",
      x: 0.5,
      y: rect.y + rect.h + 0.06,
      text: caption,
      fill: "#e7e5e4",
      fontSize: 22,
    },
    {
      type: "text",
      x: 0.5,
      y: rect.y + rect.h + 0.11,
      text: "ink: edge-trace + stipple · not a pasted photo",
      fill: "#a8a29e",
      fontSize: 12,
    },
  ];
  return strokes;
}

/** Geometric “Max” caricature — pure vectors, no network (fallback). */
export function geometricMaxStrokes(): DrawStroke[] {
  const color = "#d6d3d1";
  const cx = 0.5;
  const cy = 0.32;
  return [
    { type: "ellipse", cx, cy, rx: 0.14, ry: 0.17, stroke: color, strokeWidth: 2.5, fill: "none" },
    // hair
    {
      type: "path",
      stroke: color,
      strokeWidth: 2,
      points: [
        { x: 0.38, y: 0.28 },
        { x: 0.4, y: 0.18 },
        { x: 0.5, y: 0.14 },
        { x: 0.6, y: 0.18 },
        { x: 0.62, y: 0.28 },
      ],
    },
    // eyes
    { type: "ellipse", cx: 0.44, cy: 0.3, rx: 0.025, ry: 0.015, fill: color, stroke: color },
    { type: "ellipse", cx: 0.56, cy: 0.3, rx: 0.025, ry: 0.015, fill: color, stroke: color },
    // brows
    { type: "line", x1: 0.4, y1: 0.26, x2: 0.47, y2: 0.255, stroke: color, strokeWidth: 2 },
    { type: "line", x1: 0.53, y1: 0.255, x2: 0.6, y2: 0.26, stroke: color, strokeWidth: 2 },
    // nose
    {
      type: "path",
      stroke: color,
      strokeWidth: 1.5,
      points: [
        { x: 0.5, y: 0.3 },
        { x: 0.49, y: 0.36 },
        { x: 0.52, y: 0.37 },
      ],
    },
    // smile
    {
      type: "path",
      stroke: color,
      strokeWidth: 2,
      points: [
        { x: 0.43, y: 0.4 },
        { x: 0.47, y: 0.43 },
        { x: 0.5, y: 0.44 },
        { x: 0.53, y: 0.43 },
        { x: 0.57, y: 0.4 },
      ],
    },
    // collar
    {
      type: "path",
      stroke: color,
      strokeWidth: 2,
      points: [
        { x: 0.4, y: 0.48 },
        { x: 0.45, y: 0.52 },
        { x: 0.5, y: 0.5 },
        { x: 0.55, y: 0.52 },
        { x: 0.6, y: 0.48 },
      ],
    },
    { type: "text", x: 0.5, y: 0.62, text: "Max Glassie", fill: color, fontSize: 24 },
    {
      type: "text",
      x: 0.5,
      y: 0.68,
      text: "geometric ink · pure vectors · ui.* only",
      fill: "#a8a29e",
      fontSize: 12,
    },
  ];
}
