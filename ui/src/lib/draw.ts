/**
 * Agent pen — pure draw model for the ui.* ink overlay.
 *
 * Coordinates are NORMALIZED to the viewport (0..1), so strokes survive
 * resize. Ink is never history: it lives on the Attention/ui surface only.
 */

export type NormPoint = { readonly x: number; readonly y: number };

/** One ink primitive. All geometry in 0..1 viewport space unless noted. */
export type DrawStroke =
  | {
      readonly type: "path";
      readonly points: readonly NormPoint[];
      readonly stroke?: string;
      readonly strokeWidth?: number;
      readonly fill?: string;
      readonly closed?: boolean;
    }
  | {
      readonly type: "circle";
      readonly cx: number;
      readonly cy: number;
      readonly r: number;
      readonly stroke?: string;
      readonly strokeWidth?: number;
      readonly fill?: string;
    }
  | {
      readonly type: "ellipse";
      readonly cx: number;
      readonly cy: number;
      readonly rx: number;
      readonly ry: number;
      readonly stroke?: string;
      readonly strokeWidth?: number;
      readonly fill?: string;
    }
  | {
      readonly type: "line";
      readonly x1: number;
      readonly y1: number;
      readonly x2: number;
      readonly y2: number;
      readonly stroke?: string;
      readonly strokeWidth?: number;
    }
  | {
      readonly type: "text";
      readonly x: number;
      readonly y: number;
      readonly text: string;
      readonly fill?: string;
      readonly fontSize?: number;
    }
  | {
      readonly type: "image";
      readonly href: string;
      readonly x: number;
      readonly y: number;
      readonly w: number;
      readonly h: number;
      readonly opacity?: number;
    };

export interface DrawScene {
  readonly strokes: readonly DrawStroke[];
  /** When false, overlay is hidden but scene retained. Default true. */
  readonly visible: boolean;
  /** Optional label shown in a small badge (tour / issuer). */
  readonly label?: string;
}

export const EMPTY_SCENE: DrawScene = { strokes: [], visible: true };

const clamp01 = (n: number): number =>
  Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : 0;

function clampPoint(p: NormPoint): NormPoint {
  return { x: clamp01(p.x), y: clamp01(p.y) };
}

/** Sanitize one stroke from untrusted wire JSON. */
export function normalizeStroke(raw: unknown): DrawStroke | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const type = typeof o.type === "string" ? o.type : "";
  const num = (k: string): number | null => {
    const v = o[k];
    const n = typeof v === "number" ? v : typeof v === "string" ? Number(v) : NaN;
    return Number.isFinite(n) ? n : null;
  };
  const str = (k: string): string | undefined =>
    typeof o[k] === "string" ? (o[k] as string) : undefined;

  switch (type) {
    case "path": {
      if (!Array.isArray(o.points) || o.points.length < 2) return null;
      const points = o.points
        .map((p) => {
          if (!p || typeof p !== "object") return null;
          const pt = p as Record<string, unknown>;
          const x = typeof pt.x === "number" ? pt.x : Number(pt.x);
          const y = typeof pt.y === "number" ? pt.y : Number(pt.y);
          if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
          return clampPoint({ x, y });
        })
        .filter((p): p is NormPoint => p != null);
      if (points.length < 2) return null;
      return {
        type: "path",
        points,
        stroke: str("stroke"),
        strokeWidth: num("strokeWidth") ?? undefined,
        fill: str("fill"),
        closed: o.closed === true,
      };
    }
    case "circle": {
      const cx = num("cx");
      const cy = num("cy");
      const r = num("r");
      if (cx === null || cy === null || r === null || r <= 0) return null;
      return {
        type: "circle",
        cx: clamp01(cx),
        cy: clamp01(cy),
        r: Math.min(r, 1),
        stroke: str("stroke"),
        strokeWidth: num("strokeWidth") ?? undefined,
        fill: str("fill"),
      };
    }
    case "ellipse": {
      const cx = num("cx");
      const cy = num("cy");
      const rx = num("rx");
      const ry = num("ry");
      if (cx === null || cy === null || rx === null || ry === null || rx <= 0 || ry <= 0)
        return null;
      return {
        type: "ellipse",
        cx: clamp01(cx),
        cy: clamp01(cy),
        rx: Math.min(rx, 1),
        ry: Math.min(ry, 1),
        stroke: str("stroke"),
        strokeWidth: num("strokeWidth") ?? undefined,
        fill: str("fill"),
      };
    }
    case "line": {
      const x1 = num("x1");
      const y1 = num("y1");
      const x2 = num("x2");
      const y2 = num("y2");
      if (x1 === null || y1 === null || x2 === null || y2 === null) return null;
      return {
        type: "line",
        x1: clamp01(x1),
        y1: clamp01(y1),
        x2: clamp01(x2),
        y2: clamp01(y2),
        stroke: str("stroke"),
        strokeWidth: num("strokeWidth") ?? undefined,
      };
    }
    case "text": {
      const x = num("x");
      const y = num("y");
      const text = str("text");
      if (x === null || y === null || !text) return null;
      return {
        type: "text",
        x: clamp01(x),
        y: clamp01(y),
        text: text.slice(0, 500),
        fill: str("fill"),
        fontSize: num("fontSize") ?? undefined,
      };
    }
    case "image": {
      const href = str("href");
      const x = num("x");
      const y = num("y");
      const w = num("w");
      const h = num("h");
      if (!href || x === null || y === null || w === null || h === null) return null;
      // Only allow http(s) or data: — no javascript:
      if (!/^(https?:|data:image\/)/i.test(href)) return null;
      return {
        type: "image",
        href: href.slice(0, 4000),
        x: clamp01(x),
        y: clamp01(y),
        w: Math.min(Math.max(w, 0.01), 1),
        h: Math.min(Math.max(h, 0.01), 1),
        opacity: num("opacity") ?? undefined,
      };
    }
    default:
      return null;
  }
}

/** Normalize a batch of strokes from the wire. */
export function normalizeStrokes(raw: unknown): DrawStroke[] {
  if (!Array.isArray(raw)) return [];
  return raw.map(normalizeStroke).filter((s): s is DrawStroke => s != null);
}

/** Apply a draw intent to the current scene (pure). */
export function applyDrawIntent(
  scene: DrawScene,
  intent: {
    readonly clear?: boolean;
    readonly strokes?: readonly DrawStroke[];
    readonly visible?: boolean;
    readonly label?: string;
    /** replace = scene becomes strokes only; append = concat (default append). */
    readonly mode?: "append" | "replace";
  },
): DrawScene {
  const clear = intent.clear === true || intent.mode === "replace";
  const nextStrokes = clear
    ? [...(intent.strokes ?? [])]
    : [...scene.strokes, ...(intent.strokes ?? [])];
  return {
    strokes: nextStrokes,
    visible: intent.visible === undefined ? scene.visible : intent.visible !== false,
    label: intent.label !== undefined ? intent.label : scene.label,
  };
}

/** Classic smiley in the center of the viewport (normalized). */
export function smileyStrokes(opts?: {
  readonly cx?: number;
  readonly cy?: number;
  readonly r?: number;
  readonly color?: string;
}): DrawStroke[] {
  const cx = opts?.cx ?? 0.5;
  const cy = opts?.cy ?? 0.48;
  const r = opts?.r ?? 0.18;
  const color = opts?.color ?? "#2f4a3e";
  const eyeY = cy - r * 0.25;
  const eyeDx = r * 0.35;
  const eyeR = r * 0.08;
  // Smile as a quadratic-ish polyline arc
  const smile: NormPoint[] = [];
  for (let i = 0; i <= 12; i++) {
    const t = i / 12;
    const ang = Math.PI * 0.15 + t * Math.PI * 0.7; // lower semicircle-ish
    smile.push({
      x: cx + Math.cos(ang) * r * 0.55,
      y: cy + Math.sin(ang) * r * 0.45 + r * 0.05,
    });
  }
  return [
    { type: "circle", cx, cy, r, stroke: color, strokeWidth: 3, fill: "#f5f2eb" },
    { type: "circle", cx: cx - eyeDx, cy: eyeY, r: eyeR, fill: color, stroke: color },
    { type: "circle", cx: cx + eyeDx, cy: eyeY, r: eyeR, fill: color, stroke: color },
    { type: "path", points: smile, stroke: color, strokeWidth: 3, fill: "none" },
  ];
}

/** Path → SVG d attribute in viewBox 0 0 1000 1000. */
export function pathToSvgD(points: readonly NormPoint[], closed = false): string {
  if (points.length === 0) return "";
  const to = (p: NormPoint) => `${(p.x * 1000).toFixed(1)} ${(p.y * 1000).toFixed(1)}`;
  let d = `M ${to(points[0]!)}`;
  for (let i = 1; i < points.length; i++) d += ` L ${to(points[i]!)}`;
  if (closed) d += " Z";
  return d;
}
