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
      fill: "#cdd6f9",
      fontSize: 22,
    });
  }
  if (items.length === 0) {
    strokes.push({
      type: "text",
      x: 0.5,
      y: 0.5,
      text: "No diagram labels",
      fill: "#565f89",
      fontSize: 16,
    });
    return strokes;
  }

  const top = 0.16;
  const bottom = 0.88;
  const boxH = Math.min(0.09, (bottom - top) / items.length - 0.02);
  const INK = "#7aa2f7";      // accent — one voice for agent diagrams
  const TEXT = "#cdd6f9";     // theme text
  const CONNECT = "#565f89";  // muted connector

  for (let i = 0; i < items.length; i++) {
    const y = top + i * ((bottom - top) / items.length);
    const w = Math.min(0.64, Math.max(0.2, 0.05 + items[i]!.length * 0.013));
    const x = 0.5 - w / 2;
    strokes.push({
      type: "path",
      points: [
        { x, y },
        { x: x + w, y },
        { x: x + w, y: y + boxH },
        { x, y: y + boxH },
      ],
      closed: true,
      fill: "none",
      stroke: INK,
      strokeWidth: 2,
    });
    strokes.push({
      type: "text",
      x: 0.5,
      y: y + boxH / 2 + 0.01,
      text: items[i]!.slice(0, 36),
      fill: TEXT,
      fontSize: 18,
    });
    if (i < items.length - 1) {
      strokes.push({
        type: "line",
        x1: 0.5,
        y1: y + boxH,
        x2: 0.5,
        y2: y + (bottom - top) / items.length,
        stroke: CONNECT,
        strokeWidth: 2,
      });
    }
  }
  return strokes;
}
