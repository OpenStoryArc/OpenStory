/**
 * Layout eyes — where a thing *is on the glass* (viewport 0..1).
 *
 * History eyes say *what* to point at. Attention eyes say *which* view/session.
 * Layout eyes publish DOM bounding boxes so the agent pen can aim.
 *
 * Never history: layout rides on ui-state / interactions only.
 */

import type { DrawStroke } from "@/lib/draw";

/** Normalized viewport rect (origin top-left, x/y/w/h in 0..1). */
export type LayoutRect = {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
};

export type LayoutTarget = {
  /** Semantic kind: event | session | spotlight | view | custom */
  readonly kind: string;
  readonly id: string;
  readonly rect: LayoutRect;
  /** How we found it (for debugging). */
  readonly source?: string;
};

export type LayoutEyes = {
  readonly targets: readonly LayoutTarget[];
  /** Best single target for "draw a ring around attention". */
  readonly focus: LayoutTarget | null;
  readonly viewport: { readonly w: number; readonly h: number };
  readonly at: string;
};

const clamp01 = (n: number): number =>
  Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : 0;

/** Pure: CSS/DOM pixel rect + viewport size → unit layout rect (clipped to glass). */
export function normalizeDomRect(
  box: { left: number; top: number; width: number; height: number },
  viewport: { w: number; h: number },
): LayoutRect | null {
  const vw = viewport.w;
  const vh = viewport.h;
  if (!(vw > 0) || !(vh > 0)) return null;
  if (!(box.width > 0) || !(box.height > 0)) return null;
  // Clip the box to the viewport, then normalize — so overhang doesn't
  // claim space off-glass (left=-50,w=200 on 100-wide → visible 0..1).
  const x1 = clamp01(box.left / vw);
  const y1 = clamp01(box.top / vh);
  const x2 = clamp01((box.left + box.width) / vw);
  const y2 = clamp01((box.top + box.height) / vh);
  const w = x2 - x1;
  const h = y2 - y1;
  if (w <= 0 || h <= 0) return null;
  return { x: x1, y: y1, w, h };
}

/** Pure: expand rect by pad (unit space), still clamped. */
export function padLayoutRect(rect: LayoutRect, pad = 0.01): LayoutRect {
  const x = clamp01(rect.x - pad);
  const y = clamp01(rect.y - pad);
  const x2 = clamp01(rect.x + rect.w + pad);
  const y2 = clamp01(rect.y + rect.h + pad);
  return { x, y, w: Math.max(0, x2 - x), h: Math.max(0, y2 - y) };
}

/** Pure: pick focus among targets (prefer id, then kind priority). */
export function pickFocusTarget(
  targets: readonly LayoutTarget[],
  prefer?: { kind?: string; id?: string },
): LayoutTarget | null {
  if (targets.length === 0) return null;
  const rank = (k: string) =>
    k === "spotlight" ? 0 : k === "event" ? 1 : k === "session" ? 2 : 3;
  const best = (list: readonly LayoutTarget[]) =>
    [...list].sort((a, b) => rank(a.kind) - rank(b.kind))[0] ?? null;

  if (prefer?.id) {
    const byId = targets.filter((t) => t.id === prefer.id);
    if (byId.length > 0) return best(byId);
  }
  if (prefer?.kind) {
    const byKind = targets.filter((t) => t.kind === prefer.kind);
    if (byKind.length > 0) return best(byKind);
  }
  return best(targets);
}

/**
 * Agent pen strokes: rectangle ring around a layout rect (unit space).
 * Optional label near the top edge.
 */
export function ringStrokesForRect(
  rect: LayoutRect,
  opts?: {
    readonly pad?: number;
    readonly stroke?: string;
    readonly strokeWidth?: number;
    readonly label?: string;
    readonly labelFill?: string;
  },
): DrawStroke[] {
  const r = padLayoutRect(rect, opts?.pad ?? 0.008);
  if (r.w <= 0 || r.h <= 0) return [];
  const stroke = opts?.stroke ?? "#e11d48";
  const strokeWidth = opts?.strokeWidth ?? 2.5;
  const x1 = r.x;
  const y1 = r.y;
  const x2 = r.x + r.w;
  const y2 = r.y + r.h;
  const strokes: DrawStroke[] = [
    {
      type: "path",
      points: [
        { x: x1, y: y1 },
        { x: x2, y: y1 },
        { x: x2, y: y2 },
        { x: x1, y: y2 },
      ],
      closed: true,
      stroke,
      strokeWidth,
      fill: "none",
    },
  ];
  if (opts?.label) {
    strokes.push({
      type: "text",
      x: clamp01(r.x + r.w / 2),
      y: clamp01(Math.max(0.02, r.y - 0.015)),
      text: opts.label.slice(0, 80),
      fill: opts.labelFill ?? stroke,
      fontSize: 14,
    });
  }
  return strokes;
}

/** DOM measure: one element → LayoutTarget (null if not measurable). */
export function measureElementTarget(
  el: Element,
  viewport?: { w: number; h: number },
): LayoutTarget | null {
  const kind =
    el.getAttribute("data-os-target") ||
    (el.hasAttribute("data-event-id") ? "event" : null) ||
    (el.hasAttribute("data-session-id") ? "session" : null);
  if (!kind) return null;
  const id =
    el.getAttribute("data-os-id") ||
    el.getAttribute("data-event-id") ||
    el.getAttribute("data-session-id") ||
    "";
  if (!id) return null;
  const vw = viewport?.w ?? (typeof window !== "undefined" ? window.innerWidth : 0);
  const vh = viewport?.h ?? (typeof window !== "undefined" ? window.innerHeight : 0);
  const box = el.getBoundingClientRect();
  const rect = normalizeDomRect(
    { left: box.left, top: box.top, width: box.width, height: box.height },
    { w: vw, h: vh },
  );
  if (!rect) return null;
  // Skip near-zero / fully off-glass after normalization edge cases
  if (rect.w < 0.005 || rect.h < 0.005) return null;
  return {
    kind,
    id,
    rect,
    source: el.getAttribute("data-os-target") ? "data-os-target" : "legacy-attr",
  };
}

/**
 * Scan the document for layout targets.
 * Prefers `[data-os-target][data-os-id]`, also picks up data-event-id / data-session-id.
 */
export function collectLayoutEyes(
  opts?: {
    readonly root?: ParentNode | Document;
    readonly preferKind?: string;
    readonly preferId?: string;
    readonly viewport?: { w: number; h: number };
    readonly now?: () => string;
  },
): LayoutEyes {
  const root = opts?.root ?? (typeof document !== "undefined" ? document : null);
  const vw =
    opts?.viewport?.w ?? (typeof window !== "undefined" ? window.innerWidth : 0);
  const vh =
    opts?.viewport?.h ?? (typeof window !== "undefined" ? window.innerHeight : 0);
  const viewport = { w: vw, h: vh };
  const targets: LayoutTarget[] = [];
  const seen = new Set<string>();

  if (root) {
    const nodes = root.querySelectorAll(
      "[data-os-target][data-os-id], [data-event-id], [data-session-id]",
    );
    nodes.forEach((el) => {
      const t = measureElementTarget(el, viewport);
      if (!t) return;
      const key = `${t.kind}:${t.id}`;
      if (seen.has(key)) return;
      seen.add(key);
      targets.push(t);
    });
  }

  const focus = pickFocusTarget(targets, {
    kind: opts?.preferKind,
    id: opts?.preferId,
  });

  return {
    targets,
    focus,
    viewport,
    at: opts?.now?.() ?? new Date().toISOString(),
  };
}

/** Serialize for interaction / ui-state wire (JSON-safe plain object). */
export function layoutEyesToWire(eyes: LayoutEyes): {
  targets: LayoutTarget[];
  focus: LayoutTarget | null;
  viewport: { w: number; h: number };
  at: string;
} {
  return {
    targets: eyes.targets.map((t) => ({
      kind: t.kind,
      id: t.id,
      rect: { ...t.rect },
      ...(t.source ? { source: t.source } : {}),
    })),
    focus: eyes.focus
      ? {
          kind: eyes.focus.kind,
          id: eyes.focus.id,
          rect: { ...eyes.focus.rect },
          ...(eyes.focus.source ? { source: eyes.focus.source } : {}),
        }
      : null,
    viewport: { ...eyes.viewport },
    at: eyes.at,
  };
}
