/**
 * Async portrait recipes for the agent pen — fetch image → ink strokes.
 * Never writes history; only produces DrawStroke[] for ui.* ink.
 */

import type { DrawStroke } from "@/lib/draw";
import {
  centerCropSquare,
  dualEdgeTrace,
  filterStrokesToEllipse,
  stippleFromImageData,
  type PixelBuffer,
  type TraceRect,
} from "@/lib/draw-trace";

/** Portrait frame in viewport — slightly larger face, room for caption. */
const MAX_FACE: TraceRect = { x: 0.29, y: 0.04, w: 0.42, h: 0.5 };

/** Load URL into pixel buffer (browser). Center-crops face region. */
export async function loadImageData(
  href: string,
  maxSide = 220,
  cropFrac = 0.78,
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
  const full: PixelBuffer = { width: id.width, height: id.height, data: id.data };
  return centerCropSquare(full, cropFrac);
}

/**
 * Handsome ink portrait v0.3: crop → dual edges (RDP-smoothed) + soft stipple,
 * ellipse-masked. Pure path/circle/text ink — never a pasted photo.
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
  const data = await loadImageData(href, 220, 0.8);
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  const rx = rect.w * 0.47;
  const ry = rect.h * 0.49;

  // Contour + detail
  const edges = dualEdgeTrace(data, {
    rect,
    edgeThreshold: 95,
    softThreshold: 48,
    maxPaths: 95,
    minPathLen: 6,
    stride: 1,
    color: "#f5f5f4",
    strokeWidth: 2.35,
    softWidth: 1.0,
  });

  // Form: denser in shadows, still face-masked later
  const stipple = stippleFromImageData(data, {
    rect,
    step: 3,
    maxDots: 550,
    darkBelow: 110,
    color: "#a8a29e",
  });

  // Weight longer edge paths as "hero" lines (already mixed soft/strong)
  const ink = filterStrokesToEllipse([...stipple, ...edges], cx, cy, rx * 1.02, ry * 1.02);

  const caption = opts?.caption ?? opts?.label ?? "portrait";
  return [
    // dark matte behind face (reads as intentional portrait)
    {
      type: "ellipse",
      cx,
      cy,
      rx: rx * 1.08,
      ry: ry * 1.08,
      stroke: "none",
      strokeWidth: 0,
      fill: "#1c1917",
    },
    {
      type: "ellipse",
      cx,
      cy,
      rx: rx * 1.03,
      ry: ry * 1.03,
      stroke: "#57534e",
      strokeWidth: 3,
      fill: "none",
    },
    {
      type: "ellipse",
      cx,
      cy,
      rx,
      ry,
      stroke: "#d6d3d1",
      strokeWidth: 1.2,
      fill: "none",
    },
    ...ink,
    {
      type: "text",
      x: 0.5,
      y: Math.min(0.9, rect.y + rect.h + 0.065),
      text: caption,
      fill: "#fafaf9",
      fontSize: 28,
    },
    {
      type: "text",
      x: 0.5,
      y: Math.min(0.95, rect.y + rect.h + 0.115),
      text: "handsome mode v0.3 · crop · dual-edge · stipple",
      fill: "#a8a29e",
      fontSize: 11,
    },
  ];
}

/** Geometric “Max” caricature — pure vectors, no network (fallback). */
export function geometricMaxStrokes(): DrawStroke[] {
  const color = "#f5f5f4";
  const cx = 0.5;
  const cy = 0.32;
  return [
    { type: "ellipse", cx, cy, rx: 0.145, ry: 0.175, stroke: color, strokeWidth: 2.6, fill: "#1c1917" },
    {
      type: "path",
      stroke: color,
      strokeWidth: 2.3,
      points: [
        { x: 0.37, y: 0.28 },
        { x: 0.39, y: 0.16 },
        { x: 0.5, y: 0.12 },
        { x: 0.61, y: 0.16 },
        { x: 0.63, y: 0.28 },
      ],
    },
    { type: "ellipse", cx: 0.44, cy: 0.3, rx: 0.03, ry: 0.014, fill: color, stroke: color },
    { type: "ellipse", cx: 0.56, cy: 0.3, rx: 0.03, ry: 0.014, fill: color, stroke: color },
    { type: "line", x1: 0.395, y1: 0.252, x2: 0.47, y2: 0.248, stroke: color, strokeWidth: 2.2 },
    { type: "line", x1: 0.53, y1: 0.248, x2: 0.605, y2: 0.252, stroke: color, strokeWidth: 2.2 },
    {
      type: "path",
      stroke: color,
      strokeWidth: 1.6,
      points: [
        { x: 0.5, y: 0.3 },
        { x: 0.488, y: 0.355 },
        { x: 0.52, y: 0.368 },
      ],
    },
    {
      type: "path",
      stroke: color,
      strokeWidth: 2.4,
      points: [
        { x: 0.415, y: 0.392 },
        { x: 0.46, y: 0.432 },
        { x: 0.5, y: 0.448 },
        { x: 0.54, y: 0.432 },
        { x: 0.585, y: 0.392 },
      ],
    },
    {
      type: "path",
      stroke: color,
      strokeWidth: 2.1,
      points: [
        { x: 0.38, y: 0.47 },
        { x: 0.45, y: 0.525 },
        { x: 0.5, y: 0.5 },
        { x: 0.55, y: 0.525 },
        { x: 0.62, y: 0.47 },
      ],
    },
    { type: "text", x: 0.5, y: 0.62, text: "Max Glassie", fill: color, fontSize: 28 },
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
