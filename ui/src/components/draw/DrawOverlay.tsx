/**
 * Full-viewport SVG ink layer for agent (and human) drawing.
 *
 * Paints the CURRENT CONTEXT's glass ink (routeGlassKey → glass-ink store),
 * never the Draw tab's board — annotation is deictic, so ink lives with the
 * thing it points at and disappears when you navigate away from it.
 *
 * Default: pointer-events none so the mirror stays usable underneath.
 * Annotate mode (drawInteractive$): capture freehand on the glass
 * (story, explore, live) without leaving the tab.
 *
 * z-[100] sits above reel PlaybackClickSurface (z-55) and caption (z-60).
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { drawInteractive$ } from "@/streams/draw";
import { activeBeatKey$ } from "@/streams/reel-annotate";
import { appendGlassStrokes, glassInkStore$ } from "@/streams/glass-ink";
import { emptyGlassInkStore, routeGlassKey, type GlassInkStore } from "@/lib/glass-ink";
import type { HashRoute } from "@/lib/hash-route";
import {
  clientToUnitPoint,
  pathToSvgD,
  type DrawStroke,
  type NormPoint,
} from "@/lib/draw";
import { marginaliaPathStrokes } from "@/lib/pen-eyes";

function StrokeEl({ s }: { s: DrawStroke }) {
  const sw = s.type !== "text" && s.type !== "image" ? (s.strokeWidth ?? 2.5) : 2;
  const stroke = "stroke" in s ? s.stroke ?? "#2f4a3e" : "#2f4a3e";
  const fill = "fill" in s ? s.fill ?? "none" : "none";

  switch (s.type) {
    case "path":
      return (
        <path
          d={pathToSvgD(s.points, s.closed)}
          stroke={stroke}
          strokeWidth={sw}
          fill={s.fill ?? "none"}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      );
    case "circle":
      return (
        <circle
          cx={s.cx * 1000}
          cy={s.cy * 1000}
          r={s.r * 1000}
          stroke={stroke}
          strokeWidth={sw}
          fill={fill === "none" ? "none" : fill}
        />
      );
    case "ellipse":
      return (
        <ellipse
          cx={s.cx * 1000}
          cy={s.cy * 1000}
          rx={s.rx * 1000}
          ry={s.ry * 1000}
          stroke={stroke}
          strokeWidth={sw}
          fill={fill === "none" ? "none" : fill}
        />
      );
    case "line":
      return (
        <line
          x1={s.x1 * 1000}
          y1={s.y1 * 1000}
          x2={s.x2 * 1000}
          y2={s.y2 * 1000}
          stroke={stroke}
          strokeWidth={sw}
          strokeLinecap="round"
        />
      );
    case "text":
      return (
        <text
          x={s.x * 1000}
          y={s.y * 1000}
          fill={s.fill ?? "#2f4a3e"}
          fontSize={(s.fontSize ?? 18) * (1000 / 900)}
          fontFamily="Georgia, 'Iowan Old Style', serif"
          textAnchor="middle"
          className="pointer-events-none select-none"
        >
          {s.text}
        </text>
      );
    case "image":
      return (
        <image
          href={s.href}
          x={s.x * 1000}
          y={s.y * 1000}
          width={s.w * 1000}
          height={s.h * 1000}
          opacity={s.opacity ?? 1}
          preserveAspectRatio="xMidYMid meet"
        />
      );
    default:
      return null;
  }
}

export function DrawOverlay({
  route,
  suppress = false,
}: {
  readonly route: HashRoute;
  readonly suppress?: boolean;
}) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const ptsRef = useRef<NormPoint[]>([]);
  const drawingRef = useRef(false);
  const [store, setStore] = useState<GlassInkStore>(emptyGlassInkStore);
  const [interactive, setInteractive] = useState(false);
  /** When a reel beat owns the glass, BeatInkLayer handles ink — hide global overlay. */
  const [beatScoped, setBeatScoped] = useState(false);
  const [livePts, setLivePts] = useState<NormPoint[]>([]);

  /** The context this glass paints. null = another surface owns ink
   *  (Draw tab board, reels player beats). */
  const glassKey = routeGlassKey(route);
  const glassKeyRef = useRef(glassKey);
  glassKeyRef.current = glassKey;
  /** Derived, not stored: a navigation repaints from the same store, so ink
   *  never lingers a frame past the context it belongs to. */
  const strokes: readonly DrawStroke[] = glassKey
    ? (store.byKey[glassKey]?.strokes ?? [])
    : [];

  useEffect(() => {
    const a = glassInkStore$().subscribe(setStore);
    const b = drawInteractive$().subscribe(setInteractive);
    const c = activeBeatKey$().subscribe((k) => setBeatScoped(k != null));
    return () => {
      a.unsubscribe();
      b.unsubscribe();
      c.unsubscribe();
    };
  }, []);

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
    const key = glassKeyRef.current;
    if (pts.length >= 2 && key) {
      // High-contrast pair: dark understroke + bright yellow (readable on dark diagram beats).
      // Lands on THIS context's glass — not the Draw tab's board.
      appendGlassStrokes(key, marginaliaPathStrokes(pts));
    }
  }, []);

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

  if (suppress) return null;
  // Reel player beat layer owns the stage — don't paint global studio ink on slides.
  if (beatScoped) return null;
  // No context owns this glass (Draw tab / reels player): nothing to paint.
  if (glassKey == null && !interactive) return null;
  // Mount when annotating (even if empty) or when there is ink to show.
  if (!interactive && strokes.length === 0) return null;

  return (
    <div
      className={`fixed inset-0 z-[100] select-none ${interactive ? "cursor-crosshair" : "pointer-events-none"}`}
      style={{ pointerEvents: interactive ? "auto" : "none" }}
      data-testid="draw-overlay"
      data-glass-key={glassKey ?? ""}
      data-interactive={interactive ? "true" : "false"}
      aria-hidden={!interactive}
    >
      {interactive && (
        <div className="pointer-events-none absolute left-1/2 top-16 z-[101] -translate-x-1/2 rounded-full border-2 border-yellow-300 bg-yellow-400 px-4 py-2 text-[13px] font-semibold text-slate-900 shadow-lg">
          ✎ ANNOTATING — drag bright yellow · Done to click through
        </div>
      )}
      <svg
        ref={svgRef}
        className="h-full w-full select-none touch-none"
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
              strokeLinejoin="round"
            />
            <path
              d={pathToSvgD(livePts, false)}
              stroke="#facc15"
              strokeWidth={5}
              fill="none"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </>
        )}
      </svg>
    </div>
  );
}
