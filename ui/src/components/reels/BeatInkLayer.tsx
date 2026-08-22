/**
 * Stage-scoped freehand for the *active reel beat* only.
 * Unit coords match the diagram/title stage (full viewport meet), 1:1 with that slide.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  appendActiveBeatStrokes,
  beatInkStore$,
  getActiveBeatInk,
  setActiveBeatKey,
} from "@/streams/reel-annotate";
import { drawInteractive$ } from "@/streams/draw";
import {
  clientToUnitPoint,
  pathToSvgD,
  type DrawStroke,
  type NormPoint,
} from "@/lib/draw";
import { marginaliaPathStrokes } from "@/lib/pen-eyes";
import { beatInkToWire, beatKeyString, type BeatKey } from "@/lib/reel-annotate";
import { postInteraction } from "@/lib/interaction";

function StrokeEl({ s }: { s: DrawStroke }) {
  switch (s.type) {
    case "path":
      return (
        <path
          d={pathToSvgD(s.points, s.closed)}
          stroke={s.stroke ?? "#facc15"}
          strokeWidth={s.strokeWidth ?? 3}
          fill={s.fill ?? "none"}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      );
    case "line":
      return (
        <line
          x1={s.x1 * 1000}
          y1={s.y1 * 1000}
          x2={s.x2 * 1000}
          y2={s.y2 * 1000}
          stroke={s.stroke ?? "#facc15"}
          strokeWidth={s.strokeWidth ?? 3}
          strokeLinecap="round"
        />
      );
    case "circle":
      return (
        <circle
          cx={s.cx * 1000}
          cy={s.cy * 1000}
          r={s.r * 1000}
          stroke={s.stroke ?? "#facc15"}
          strokeWidth={s.strokeWidth ?? 3}
          fill={s.fill ?? "none"}
        />
      );
    case "text":
      return (
        <text
          x={s.x * 1000}
          y={s.y * 1000}
          fill={s.fill ?? "#facc15"}
          fontSize={s.fontSize ?? 18}
          textAnchor="middle"
          className="pointer-events-none"
        >
          {s.text}
        </text>
      );
    default:
      return null;
  }
}

function reportBeatInk(key: BeatKey, interactive: boolean): void {
  const ink = getActiveBeatInk();
  if (!ink) return;
  postInteraction({
    kind: "navigate",
    view: "reels",
    reelId: key.reelId,
    annotate: interactive,
    beatIndex: key.beatIndex,
    beatInk: beatInkToWire(ink, { interactive }),
  });
}

export function BeatInkLayer({
  reelId,
  beatIndex,
}: {
  readonly reelId: string;
  readonly beatIndex: number;
}) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const ptsRef = useRef<NormPoint[]>([]);
  const drawingRef = useRef(false);
  const [interactive, setInteractive] = useState(false);
  const [strokes, setStrokes] = useState<readonly DrawStroke[]>([]);
  const [livePts, setLivePts] = useState<NormPoint[]>([]);
  const key: BeatKey = { reelId, beatIndex };

  useEffect(() => {
    setActiveBeatKey(key);
    setStrokes(getActiveBeatInk()?.strokes ?? []);
    return () => setActiveBeatKey(null);
  }, [reelId, beatIndex]);

  useEffect(() => {
    const a = drawInteractive$().subscribe((on) => {
      setInteractive(on);
      reportBeatInk(key, on);
    });
    const b = beatInkStore$().subscribe((store) => {
      const row = store.byKey[beatKeyString(key)];
      setStrokes(row?.strokes ?? []);
    });
    return () => {
      a.unsubscribe();
      b.unsubscribe();
    };
  }, [reelId, beatIndex]);

  const toNorm = useCallback((clientX: number, clientY: number): NormPoint | null => {
    const el = svgRef.current;
    if (!el) return null;
    return clientToUnitPoint(clientX, clientY, el.getBoundingClientRect(), { fit: "meet" });
  }, []);

  const endStroke = useCallback(() => {
    if (!drawingRef.current) return;
    drawingRef.current = false;
    const pts = ptsRef.current;
    ptsRef.current = [];
    setLivePts([]);
    if (pts.length < 2) return;
    appendActiveBeatStrokes(marginaliaPathStrokes(pts));
    reportBeatInk(key, true);
  }, [key]);

  const onPointerDown = (e: React.PointerEvent) => {
    if (!interactive) return;
    e.preventDefault();
    e.stopPropagation();
    const p = toNorm(e.clientX, e.clientY);
    if (!p) return;
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    drawingRef.current = true;
    ptsRef.current = [p];
    setLivePts([p]);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!interactive || !drawingRef.current) return;
    e.preventDefault();
    e.stopPropagation();
    const p = toNorm(e.clientX, e.clientY);
    if (!p) return;
    ptsRef.current = [...ptsRef.current, p];
    setLivePts(ptsRef.current);
  };

  const onPointerUp = (e: React.PointerEvent) => {
    if (!interactive) return;
    e.preventDefault();
    e.stopPropagation();
    endStroke();
  };

  return (
    <div
      className={`fixed inset-0 z-[100] select-none ${interactive ? "cursor-crosshair" : "pointer-events-none"}`}
      style={{ pointerEvents: interactive ? "auto" : "none" }}
      data-testid="beat-ink-layer"
      data-beat={beatKeyString(key)}
      data-interactive={interactive ? "true" : "false"}
    >
      {interactive && (
        <div className="pointer-events-none absolute left-1/2 top-16 z-[101] -translate-x-1/2 rounded-full border-2 border-yellow-300 bg-yellow-400 px-4 py-2 text-[13px] font-semibold text-slate-900 shadow-lg">
          ✎ Slide {beatIndex + 1} only — yellow ink is tied to this beat
        </div>
      )}
      <svg
        ref={svgRef}
        className="h-full w-full touch-none select-none"
        style={{ pointerEvents: interactive ? "auto" : "none" }}
        viewBox="0 0 1000 1000"
        preserveAspectRatio="xMidYMid meet"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      >
        {strokes.map((s, i) => (
          <StrokeEl key={i} s={s} />
        ))}
        {livePts.length >= 2 && (
          <>
            <path
              d={pathToSvgD(livePts, false)}
              stroke="#0f172a"
              strokeWidth={10}
              fill="none"
              strokeLinecap="round"
            />
            <path
              d={pathToSvgD(livePts, false)}
              stroke="#facc15"
              strokeWidth={5}
              fill="none"
              strokeLinecap="round"
            />
          </>
        )}
      </svg>
    </div>
  );
}
