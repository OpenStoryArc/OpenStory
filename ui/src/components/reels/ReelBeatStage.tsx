/**
 * Full-screen stage for non-spotlight reel beats (title / diagram / image).
 * Spotlight still uses EventSpotlight. Pure presentational + optional fetch.
 */

import { useEffect, useState } from "react";
import type { ReelStop } from "@/lib/reels-api";
import { diagramLabelsToStrokes, normalizeStopKind } from "@/lib/reel-visual";
import { pathToSvgD } from "@/lib/draw";
import type { DrawStroke } from "@/lib/draw";

export function ReelBeatStage({
  stop,
  onClose,
}: {
  readonly stop: ReelStop;
  readonly onClose: () => void;
}) {
  const kind = normalizeStopKind(stop.kind);
  const [strokes, setStrokes] = useState<DrawStroke[]>([]);

  useEffect(() => {
    if (kind !== "diagram") return;
    const labels = stop.visual?.labels;
    if (labels && labels.length > 0) {
      setStrokes(
        diagramLabelsToStrokes(labels, { title: stop.visual?.title ?? "Diagram" }),
      );
      return;
    }
    const sid = stop.visual?.sessionId || stop.sessionId;
    if (!sid) {
      setStrokes(diagramLabelsToStrokes([], { title: "Diagram" }));
      return;
    }
    let cancelled = false;
    fetch(`/api/sessions/${encodeURIComponent(sid)}/tool-journey`)
      .then((r) => (r.ok ? r.json() : []))
      .then((rows: unknown) => {
        if (cancelled) return;
        const list = Array.isArray(rows) ? rows : [];
        const tools = list
          .map((e) =>
            e && typeof e === "object" && "tool" in e
              ? String((e as { tool?: string }).tool ?? "")
              : "",
          )
          .filter(Boolean);
        // light collapse consecutive
        const collapsed: string[] = [];
        for (const t of tools) {
          const prev = collapsed[collapsed.length - 1];
          if (prev && prev.startsWith(t)) {
            const m = prev.match(/×(\d+)$/);
            const n = m ? Number(m[1]) + 1 : 2;
            collapsed[collapsed.length - 1] = `${t} ×${n}`;
          } else if (prev === t) {
            collapsed[collapsed.length - 1] = `${t} ×2`;
          } else {
            collapsed.push(t);
          }
        }
        setStrokes(
          diagramLabelsToStrokes(collapsed.slice(0, 8), {
            title: stop.visual?.title ?? "Tool journey",
          }),
        );
      })
      .catch(() => {
        if (!cancelled) setStrokes(diagramLabelsToStrokes([], { title: "Diagram" }));
      });
    return () => {
      cancelled = true;
    };
  }, [kind, stop]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  if (kind === "title") {
    return (
      <div
        className="spotlight-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm"
        data-testid="reel-beat-title"
        onClick={onClose}
      >
        <p className="mx-8 max-w-3xl text-center text-3xl font-semibold leading-snug text-white md:text-4xl">
          {stop.line}
        </p>
      </div>
    );
  }

  if (kind === "image" && stop.visual?.imageHref) {
    return (
      <div
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/80"
        data-testid="reel-beat-image"
        onClick={onClose}
      >
        <img
          src={stop.visual.imageHref}
          alt={stop.line}
          className="max-h-[80vh] max-w-[90vw] rounded-lg object-contain shadow-2xl"
        />
      </div>
    );
  }

  // diagram (default non-spotlight fallback)
  return (
    <div
      className="fixed inset-0 z-50 flex flex-col bg-[#1a1b26]"
      data-testid="reel-beat-diagram"
    >
      <svg
        className="h-full w-full select-none"
        viewBox="0 0 1000 1000"
        preserveAspectRatio="xMidYMid meet"
      >
        <rect width="1000" height="1000" fill="#1a1b26" />
        {strokes.map((s, i) => {
          if (s.type === "path") {
            return (
              <path
                key={i}
                d={pathToSvgD(s.points, s.closed)}
                stroke={s.stroke ?? "#94a3b8"}
                strokeWidth={s.strokeWidth ?? 2}
                fill={s.fill ?? "none"}
              />
            );
          }
          if (s.type === "line") {
            return (
              <line
                key={i}
                x1={s.x1 * 1000}
                y1={s.y1 * 1000}
                x2={s.x2 * 1000}
                y2={s.y2 * 1000}
                stroke={s.stroke ?? "#64748b"}
                strokeWidth={s.strokeWidth ?? 2}
              />
            );
          }
          if (s.type === "text") {
            return (
              <text
                key={i}
                x={s.x * 1000}
                y={s.y * 1000}
                fill={s.fill ?? "#e2e8f0"}
                fontSize={s.fontSize ?? 16}
                textAnchor="middle"
                className="pointer-events-none"
              >
                {s.text}
              </text>
            );
          }
          return null;
        })}
      </svg>
    </div>
  );
}
