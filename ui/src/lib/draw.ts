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

/**
 * Map browser client coordinates → unit [0,1] drawing space.
 *
 * Must match SVG `viewBox="0 0 vbW vbH"` + `preserveAspectRatio`.
 * With `xMidYMid meet`, the content is letterboxed — naive
 * `(client - left) / width` drifts from the cursor (the harden bug).
 */
export function clientToUnitPoint(
  clientX: number,
  clientY: number,
  rect: { readonly left: number; readonly top: number; readonly width: number; readonly height: number },
  opts?: {
    readonly viewBoxW?: number;
    readonly viewBoxH?: number;
    /** "none" stretches; "meet" letterboxes (xMidYMid). Default meet. */
    readonly fit?: "none" | "meet";
  },
): NormPoint | null {
  if (!(rect.width > 0) || !(rect.height > 0)) return null;
  const vbW = opts?.viewBoxW ?? 1000;
  const vbH = opts?.viewBoxH ?? 1000;
  const fit = opts?.fit ?? "meet";

  if (fit === "none") {
    return {
      x: clamp01((clientX - rect.left) / rect.width),
      y: clamp01((clientY - rect.top) / rect.height),
    };
  }

  const scale = Math.min(rect.width / vbW, rect.height / vbH);
  if (!(scale > 0)) return null;
  const contentW = vbW * scale;
  const contentH = vbH * scale;
  const ox = rect.left + (rect.width - contentW) / 2;
  const oy = rect.top + (rect.height - contentH) / 2;
  const x = (clientX - ox) / contentW;
  const y = (clientY - oy) / contentH;
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x: clamp01(x), y: clamp01(y) };
}

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

/**
 * Dense botanical flower (vector illustration, not photo).
 * Layered filled petals, veins, ribbon stem, seed disc, soft shadow.
 */
export function flowerStrokes(opts?: {
  readonly cx?: number;
  readonly cy?: number;
  readonly caption?: string;
}): DrawStroke[] {
  const cx = opts?.cx ?? 0.5;
  const cy = opts?.cy ?? 0.36;
  const caption = opts?.caption ?? "for you";
  const strokes: DrawStroke[] = [];

  // Ground shadow
  strokes.push({
    type: "ellipse",
    cx: 0.5,
    cy: 0.9,
    rx: 0.14,
    ry: 0.025,
    fill: "rgba(0,0,0,0.1)",
    stroke: "transparent",
    strokeWidth: 0,
  });

  // Ribbon stem (filled thick path via dual edge lines + mid fill as wide stroke)
  const stemPts: NormPoint[] = [];
  for (let t = 0; t <= 40; t++) {
    const u = t / 40;
    stemPts.push({
      x: 0.5 + 0.025 * Math.sin(u * Math.PI * 1.6),
      y: 0.48 + u * 0.4,
    });
  }
  strokes.push({
    type: "path",
    points: stemPts,
    stroke: "#166534",
    strokeWidth: 7,
    fill: "none",
  });
  strokes.push({
    type: "path",
    points: stemPts,
    stroke: "#22c55e",
    strokeWidth: 3.5,
    fill: "none",
  });

  // Leaf helper
  const leaf = (
    x0: number,
    y0: number,
    length: number,
    side: 1 | -1,
    fill: string,
  ): DrawStroke[] => {
    const pts: NormPoint[] = [];
    for (let t = 0; t <= 24; t++) {
      const u = t / 24;
      const lx = x0 + side * u * length;
      const ly = y0 + 0.03 * u - Math.sin(u * Math.PI) * 0.05;
      const w = Math.sin(u * Math.PI) * 0.042;
      pts.push({ x: lx, y: ly - w });
    }
    for (let t = 23; t >= 0; t--) {
      const u = t / 24;
      const lx = x0 + side * u * length;
      const ly = y0 + 0.03 * u - Math.sin(u * Math.PI) * 0.05;
      const w = Math.sin(u * Math.PI) * 0.042;
      pts.push({ x: lx, y: ly + w });
    }
    const midrib: NormPoint[] = [];
    for (let t = 0; t <= 12; t++) {
      const u = t / 12;
      midrib.push({
        x: x0 + side * u * length * 0.92,
        y: y0 + 0.03 * u - Math.sin(u * Math.PI) * 0.05,
      });
    }
    const out: DrawStroke[] = [
      {
        type: "path",
        points: pts,
        closed: true,
        fill,
        stroke: "#14532d",
        strokeWidth: 1.2,
      },
      {
        type: "path",
        points: midrib,
        stroke: "#166534",
        strokeWidth: 1,
        fill: "none",
      },
    ];
    // side veins
    for (const u of [0.3, 0.5, 0.7]) {
      const bx = x0 + side * u * length * 0.92;
      const by = y0 + 0.03 * u - Math.sin(u * Math.PI) * 0.05;
      const w = Math.sin(u * Math.PI) * 0.032;
      out.push({
        type: "line",
        x1: bx,
        y1: by,
        x2: bx + side * 0.01,
        y2: by - w,
        stroke: "#15803d",
        strokeWidth: 0.8,
      });
      out.push({
        type: "line",
        x1: bx,
        y1: by,
        x2: bx + side * 0.01,
        y2: by + w,
        stroke: "#15803d",
        strokeWidth: 0.8,
      });
    }
    return out;
  };
  strokes.push(...leaf(0.5, 0.66, 0.16, -1, "#4ade80"));
  strokes.push(...leaf(0.5, 0.72, 0.15, 1, "#22c55e"));

  // Petal rings — back (darker) then front (lighter), irregular lengths
  const makePetal = (
    i: number,
    n: number,
    len: number,
    width: number,
    fill: string,
    stroke: string,
    jitter: number,
  ): DrawStroke => {
    const a = (i / n) * Math.PI * 2 - Math.PI / 2 + jitter;
    const perp = a + Math.PI / 2;
    const points: NormPoint[] = [];
    for (let t = 0; t <= 22; t++) {
      const u = t / 22;
      const w = Math.sin(u * Math.PI) * width;
      const along = len * (0.15 * u + 0.85 * u * u);
      points.push({
        x: cx + along * Math.cos(a) + w * Math.cos(perp),
        y: cy + along * Math.sin(a) * 0.92 + w * Math.sin(perp) * 0.85,
      });
    }
    for (let t = 21; t >= 0; t--) {
      const u = t / 22;
      const w = Math.sin(u * Math.PI) * width;
      const along = len * (0.15 * u + 0.85 * u * u);
      points.push({
        x: cx + along * Math.cos(a) - w * Math.cos(perp),
        y: cy + along * Math.sin(a) * 0.92 - w * Math.sin(perp) * 0.85,
      });
    }
    return {
      type: "path",
      points,
      closed: true,
      fill,
      stroke,
      strokeWidth: 1.2,
    };
  };

  // Back ring (12 petals, longer, deeper pink)
  for (let i = 0; i < 12; i++) {
    const j = ((i * 7) % 5) * 0.02 - 0.04;
    strokes.push(
      makePetal(i, 12, 0.2 + (i % 3) * 0.012, 0.038, i % 2 === 0 ? "#f9a8d4" : "#f472b6", "#be185d", j),
    );
  }
  // Front ring (8 petals, shorter, lighter)
  for (let i = 0; i < 8; i++) {
    const j = ((i * 3) % 4) * 0.015 - 0.02;
    strokes.push(
      makePetal(i + 0.5, 8, 0.145 + (i % 2) * 0.01, 0.032, i % 2 === 0 ? "#fce7f3" : "#fbcfe8", "#db2777", j),
    );
  }

  // Petal veins
  for (let i = 0; i < 8; i++) {
    const a = ((i + 0.5) / 8) * Math.PI * 2 - Math.PI / 2;
    strokes.push({
      type: "line",
      x1: cx + 0.04 * Math.cos(a),
      y1: cy + 0.04 * Math.sin(a) * 0.92,
      x2: cx + 0.13 * Math.cos(a),
      y2: cy + 0.13 * Math.sin(a) * 0.92,
      stroke: "rgba(190,24,93,0.35)",
      strokeWidth: 0.9,
    });
  }

  // Disc shadow under center
  strokes.push({
    type: "ellipse",
    cx,
    cy: cy + 0.012,
    rx: 0.07,
    ry: 0.055,
    fill: "rgba(180,83,9,0.25)",
    stroke: "transparent",
    strokeWidth: 0,
  });

  // Concentric disc
  strokes.push({
    type: "circle",
    cx,
    cy,
    r: 0.07,
    fill: "#f59e0b",
    stroke: "#b45309",
    strokeWidth: 1.5,
  });
  strokes.push({
    type: "circle",
    cx,
    cy,
    r: 0.052,
    fill: "#fbbf24",
    stroke: "#d97706",
    strokeWidth: 1,
  });
  strokes.push({
    type: "circle",
    cx,
    cy,
    r: 0.032,
    fill: "#fde68a",
    stroke: "transparent",
    strokeWidth: 0,
  });

  // Seed stipple
  for (let i = 0; i < 28; i++) {
    const a = (i / 28) * Math.PI * 2 + (i % 3) * 0.1;
    const rad = 0.012 + (i % 5) * 0.008;
    strokes.push({
      type: "circle",
      cx: cx + rad * Math.cos(a),
      cy: cy + rad * Math.sin(a) * 0.9,
      r: 0.004 + (i % 3) * 0.0015,
      fill: i % 2 === 0 ? "#92400e" : "#b45309",
      stroke: "transparent",
      strokeWidth: 0,
    });
  }

  // Soft sparkles
  for (const [sx, sy, sc] of [
    [0.22, 0.18, 0.018],
    [0.78, 0.22, 0.014],
    [0.72, 0.48, 0.012],
  ] as const) {
    strokes.push({
      type: "path",
      points: [
        { x: sx, y: sy - sc },
        { x: sx + sc * 0.2, y: sy - sc * 0.2 },
        { x: sx + sc, y: sy },
        { x: sx + sc * 0.2, y: sy + sc * 0.2 },
        { x: sx, y: sy + sc },
        { x: sx - sc * 0.2, y: sy + sc * 0.2 },
        { x: sx - sc, y: sy },
        { x: sx - sc * 0.2, y: sy - sc * 0.2 },
      ],
      closed: true,
      fill: "#fde68a",
      stroke: "#f59e0b",
      strokeWidth: 0.8,
    });
  }

  strokes.push({
    type: "text",
    x: 0.5,
    y: 0.96,
    text: caption,
    fill: "#9d174d",
    fontSize: 20,
  });

  return strokes;
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
