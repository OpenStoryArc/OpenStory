/**
 * Pure helpers for rich reel beats — diagram labels → unit-space strokes
 * (same language as the agent pen). No I/O.
 */

import type { DrawStroke } from "@/lib/draw";

export type ReelStopKind = "spotlight" | "title" | "diagram" | "image";

export function normalizeStopKind(raw: unknown): ReelStopKind {
  if (raw === "title" || raw === "diagram" || raw === "image" || raw === "spotlight") {
    return raw;
  }
  return "spotlight";
}

/** Does this stop need a real (sessionId, eventId) pair? */
export function stopRequiresEventAnchor(kind: ReelStopKind): boolean {
  return kind === "spotlight";
}

/**
 * Vertical layered boxes from journey labels — same geometry spirit as
 * scripts/diagram_hands layered layout, pure for Vitest.
 */
export function diagramLabelsToStrokes(
  labels: readonly string[],
  opts?: { readonly title?: string; readonly max?: number },
): DrawStroke[] {
  const max = opts?.max ?? 8;
  const items = labels.map((l) => l.trim()).filter(Boolean).slice(0, max);
  const strokes: DrawStroke[] = [];
  if (opts?.title) {
    strokes.push({
      type: "text",
      x: 0.5,
      y: 0.08,
      text: opts.title.slice(0, 48),
      fill: "#e2e8f0",
      fontSize: 22,
    });
  }
  if (items.length === 0) {
    strokes.push({
      type: "text",
      x: 0.5,
      y: 0.5,
      text: "No diagram labels",
      fill: "#94a3b8",
      fontSize: 16,
    });
    return strokes;
  }

  const top = 0.16;
  const bottom = 0.88;
  const boxH = Math.min(0.09, (bottom - top) / items.length - 0.02);
  const colors = [
    { fill: "#1e3a5f", stroke: "#93c5fd" },
    { fill: "#4c1d95", stroke: "#c4b5fd" },
    { fill: "#7c2d12", stroke: "#fdba74" },
    { fill: "#14532d", stroke: "#86efac" },
    { fill: "#9d174d", stroke: "#f9a8d4" },
  ];

  for (let i = 0; i < items.length; i++) {
    const y = top + i * ((bottom - top) / items.length);
    const x = 0.18;
    const w = 0.64;
    const c = colors[i % colors.length]!;
    strokes.push({
      type: "path",
      points: [
        { x, y },
        { x: x + w, y },
        { x: x + w, y: y + boxH },
        { x, y: y + boxH },
      ],
      closed: true,
      fill: c.fill,
      stroke: c.stroke,
      strokeWidth: 2,
    });
    strokes.push({
      type: "text",
      x: 0.5,
      y: y + boxH / 2 + 0.01,
      text: items[i]!.slice(0, 36),
      fill: c.stroke,
      fontSize: 16,
    });
    if (i < items.length - 1) {
      const x1 = 0.5;
      const y1 = y + boxH;
      const y2 = y + (bottom - top) / items.length;
      strokes.push({
        type: "line",
        x1,
        y1,
        x2: x1,
        y2,
        stroke: "#64748b",
        strokeWidth: 2,
      });
    }
  }
  return strokes;
}
