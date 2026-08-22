/**
 * Pen eyes — let agents *see* the ui.* ink scene (what is on the pen).
 *
 * Human/agent strokes live in draw$ (browser). Pen eyes serialize a bounded
 * snapshot onto interactions → ui-state so where_is_user can include `pen`.
 * Still never history (events.*) — openstory-ui viewing session only.
 */

import type { DrawScene, DrawStroke, NormPoint } from "@/lib/draw";

/** Wire shape agents read via where_is_user / GET /api/ui-state. */
export type PenSceneWire = {
  readonly stroke_count: number;
  readonly visible: boolean;
  readonly label?: string;
  readonly empty: boolean;
  /** True when human is in Annotate mode (glass freehand over reels/story). */
  readonly interactive?: boolean;
  /** Coarse kind histogram for quick summaries. */
  readonly kinds: Readonly<Record<string, number>>;
  /** Bounding box of all geometry in unit space (if any). */
  readonly bounds: {
    readonly x: number;
    readonly y: number;
    readonly w: number;
    readonly h: number;
  } | null;
  /** Truncated stroke list (capped) — enough to reconstruct simple diagrams. */
  readonly strokes: readonly DrawStroke[];
  /** True when strokes were truncated for size. */
  readonly truncated: boolean;
  readonly at: string;
};

export type PenSceneWireOpts = {
  readonly maxStrokes?: number;
  readonly maxPointsPerPath?: number;
  readonly maxTextLen?: number;
  readonly now?: () => string;
  readonly interactive?: boolean;
};

const DEFAULT_MAX_STROKES = 120;
const DEFAULT_MAX_POINTS = 64;
const DEFAULT_MAX_TEXT = 200;

const clamp01 = (n: number): number =>
  Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : 0;

function downsamplePoints(
  points: readonly NormPoint[],
  max: number,
): NormPoint[] {
  if (points.length <= max) return points.map((p) => ({ x: p.x, y: p.y }));
  if (max < 2) return [{ ...points[0]! }];
  const out: NormPoint[] = [];
  for (let i = 0; i < max; i++) {
    const idx =
      i === max - 1 ? points.length - 1 : Math.round((i / (max - 1)) * (points.length - 1));
    const p = points[idx]!;
    out.push({ x: p.x, y: p.y });
  }
  return out;
}

function clipStroke(
  s: DrawStroke,
  maxPoints: number,
  maxText: number,
): DrawStroke {
  switch (s.type) {
    case "path":
      return {
        type: "path",
        points: downsamplePoints(s.points, maxPoints),
        stroke: s.stroke,
        strokeWidth: s.strokeWidth,
        fill: s.fill,
        closed: s.closed,
      };
    case "text":
      return {
        type: "text",
        x: s.x,
        y: s.y,
        text: s.text.slice(0, maxText),
        fill: s.fill,
        fontSize: s.fontSize,
      };
    case "image":
      return {
        type: "image",
        href: s.href.slice(0, 500),
        x: s.x,
        y: s.y,
        w: s.w,
        h: s.h,
        opacity: s.opacity,
      };
    default:
      return s;
  }
}

/** Pure: axis-aligned bounds of a stroke list in unit space. */
export function strokeBounds(
  strokes: readonly DrawStroke[],
): { x: number; y: number; w: number; h: number } | null {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  const hit = (x: number, y: number) => {
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  };
  for (const s of strokes) {
    switch (s.type) {
      case "path":
        for (const p of s.points) hit(p.x, p.y);
        break;
      case "circle":
        hit(s.cx - s.r, s.cy - s.r);
        hit(s.cx + s.r, s.cy + s.r);
        break;
      case "ellipse":
        hit(s.cx - s.rx, s.cy - s.ry);
        hit(s.cx + s.rx, s.cy + s.ry);
        break;
      case "line":
        hit(s.x1, s.y1);
        hit(s.x2, s.y2);
        break;
      case "text":
        hit(s.x, s.y);
        break;
      case "image":
        hit(s.x, s.y);
        hit(s.x + s.w, s.y + s.h);
        break;
    }
  }
  if (!Number.isFinite(minX)) return null;
  return {
    x: clamp01(minX),
    y: clamp01(minY),
    w: clamp01(maxX - minX),
    h: clamp01(maxY - minY),
  };
}

/** Pure: kind histogram. */
export function strokeKindHistogram(
  strokes: readonly DrawStroke[],
): Record<string, number> {
  const kinds: Record<string, number> = {};
  for (const s of strokes) {
    kinds[s.type] = (kinds[s.type] ?? 0) + 1;
  }
  return kinds;
}

/** Pure: DrawScene → bounded wire payload for ui-state. */
export function penSceneToWire(
  scene: DrawScene,
  opts?: PenSceneWireOpts,
): PenSceneWire {
  const maxStrokes = opts?.maxStrokes ?? DEFAULT_MAX_STROKES;
  const maxPoints = opts?.maxPointsPerPath ?? DEFAULT_MAX_POINTS;
  const maxText = opts?.maxTextLen ?? DEFAULT_MAX_TEXT;
  const all = scene.strokes;
  const truncated = all.length > maxStrokes;
  const slice = all.slice(0, maxStrokes).map((s) => clipStroke(s, maxPoints, maxText));
  return {
    stroke_count: all.length,
    visible: scene.visible !== false,
    ...(opts?.interactive !== undefined ? { interactive: opts.interactive } : {}),
    label: scene.label,
    empty: all.length === 0,
    kinds: strokeKindHistogram(all),
    bounds: strokeBounds(all),
    strokes: slice,
    truncated,
    at: opts?.now?.() ?? new Date().toISOString(),
  };
}

/** One-line agent summary. */
export function summarizePenWire(pen: PenSceneWire | null | undefined): string {
  if (!pen || pen.empty) {
    return pen?.interactive ? "pen empty (annotating)" : "pen empty";
  }
  const kinds = Object.entries(pen.kinds)
    .map(([k, n]) => `${n} ${k}`)
    .join(", ");
  const label = pen.label ? ` “${pen.label}”` : "";
  const trunc = pen.truncated ? " (truncated)" : "";
  const ann = pen.interactive ? " · annotating" : "";
  return `pen ${pen.stroke_count} stroke(s)${label}: ${kinds || "—"}${trunc}${ann}`;
}

/**
 * High-visibility marginalia stroke pair (dark under + bright yellow over).
 * Readable on dark diagram beats and light title cards.
 */
export function marginaliaPathStrokes(
  points: readonly NormPoint[],
): DrawStroke[] {
  if (points.length < 2) return [];
  return [
    {
      type: "path",
      points,
      stroke: "#0f172a",
      strokeWidth: 10,
      fill: "none",
    },
    {
      type: "path",
      points,
      stroke: "#facc15",
      strokeWidth: 5,
      fill: "none",
    },
  ];
}
