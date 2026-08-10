/**
 * Draw tab — attention canvas paper.
 * Same scene as the global overlay; freehand = marginalia (append).
 * Clear empties; Hide keeps strokes for navigate-without-wipe.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  clearDraw,
  commitDraw,
  drawScene$,
  setDrawVisible,
} from "@/streams/draw";
import { clientToUnitPoint, pathToSvgD, type DrawScene, type NormPoint } from "@/lib/draw";

const INK = "#2f4a3e";

type InkMode = "draw" | "type";

export function DrawView() {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const [scene, setScene] = useState<DrawScene>({ strokes: [], visible: true });
  const [drawing, setDrawing] = useState(false);
  const [pts, setPts] = useState<NormPoint[]>([]);
  const [mode, setMode] = useState<InkMode>("draw");
  const [typeDraft, setTypeDraft] = useState<{
    x: number;
    y: number;
    text: string;
    clientX: number;
    clientY: number;
  } | null>(null);

  useEffect(() => {
    const sub = drawScene$().subscribe(setScene);
    return () => sub.unsubscribe();
  }, []);

  const toNorm = useCallback((clientX: number, clientY: number): NormPoint | null => {
    const el = svgRef.current;
    if (!el) return null;
    const r = el.getBoundingClientRect();
    // Must match preserveAspectRatio="xMidYMid meet" on the canvas SVG.
    return clientToUnitPoint(clientX, clientY, r, { fit: "meet" });
  }, []);

  const commitTypeDraft = useCallback(() => {
    if (!typeDraft) return;
    const t = typeDraft.text.trim();
    if (t) {
      commitDraw({
        strokes: [
          {
            type: "text",
            x: typeDraft.x,
            y: typeDraft.y,
            text: t.slice(0, 200),
            fill: INK,
            fontSize: 18,
          },
        ],
        label: "typed",
        visible: true,
      });
    }
    setTypeDraft(null);
  }, [typeDraft]);

  const onPointerDown = (e: React.PointerEvent) => {
    e.preventDefault();
    const p = toNorm(e.clientX, e.clientY);
    if (!p) return;
    if (mode === "type") {
      if (typeDraft?.text.trim()) commitTypeDraft();
      setTypeDraft({
        x: p.x,
        y: p.y,
        text: "",
        clientX: e.clientX,
        clientY: e.clientY,
      });
      return;
    }
    (e.target as Element).setPointerCapture?.(e.pointerId);
    setDrawing(true);
    setPts([p]);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!drawing) return;
    const p = toNorm(e.clientX, e.clientY);
    if (!p) return;
    setPts((prev) => [...prev, p]);
  };

  const onPointerUp = () => {
    if (!drawing) return;
    setDrawing(false);
    if (pts.length >= 2) {
      commitDraw({
        strokes: [{ type: "path", points: pts, stroke: INK, strokeWidth: 3 }],
        label: "marginalia",
        // keep visible true when user is inking
        visible: true,
      });
    }
    setPts([]);
  };

  const n = scene.strokes.length;
  const hidden = scene.visible === false;

  return (
    <div className="flex flex-1 min-h-0 flex-col" data-testid="draw-view">
      <div className="flex flex-wrap items-center gap-2 border-b border-[color:var(--border)] bg-[color:var(--bg-surface)] px-4 py-2">
        <div className="min-w-0 flex-1">
          <p className="text-sm text-[color:var(--text)]">
            Attention canvas
            <span className="text-[color:var(--text-muted)]">
              {" "}
              · ui.* only · never history
            </span>
          </p>
          <p className="text-[11px] text-[color:var(--text-muted)]">
            Draw freehand marginalia; agents ink via control. Navigate other tabs —
            ink stays (Hide to read without clearing).
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <span
            className="rounded-full border border-[color:var(--border)] px-2 py-0.5 text-[11px] text-[color:var(--text-muted)]"
            data-testid="draw-stroke-count"
          >
            {n} stroke{n === 1 ? "" : "s"}
            {scene.label ? ` · ${scene.label}` : ""}
            {hidden ? " · hidden on overlay" : ""}
          </span>
          <div className="flex rounded border border-[color:var(--border)] text-xs">
            <button
              type="button"
              className={`px-2 py-1 ${mode === "draw" ? "bg-[color:var(--accent)]/15 text-[color:var(--accent)]" : ""}`}
              onClick={() => {
                setMode("draw");
                setTypeDraft(null);
              }}
              data-testid="draw-mode-pen"
            >
              Pen
            </button>
            <button
              type="button"
              className={`border-l border-[color:var(--border)] px-2 py-1 ${mode === "type" ? "bg-[color:var(--accent)]/15 text-[color:var(--accent)]" : ""}`}
              onClick={() => setMode("type")}
              data-testid="draw-mode-type"
              title="Click the board to type a label"
            >
              Type
            </button>
          </div>
          <button
            type="button"
            className="rounded border border-[color:var(--border)] px-2 py-1 text-xs hover:border-[color:var(--accent)]"
            onClick={() => setDrawVisible(hidden)}
            data-testid="draw-hide-toggle"
            title="Hide ink on other tabs without clearing the board"
          >
            {hidden ? "Show on glass" : "Hide on glass"}
          </button>
          <button
            type="button"
            className="rounded border border-[color:var(--border)] px-2 py-1 text-xs hover:border-[color:var(--accent)]"
            onClick={() => clearDraw()}
            data-testid="draw-clear"
            title="Clear all attention ink"
          >
            Clear
          </button>
        </div>
      </div>
      <div className="relative flex-1 min-h-0 bg-white select-none">
        {hidden && n > 0 && (
          <div className="pointer-events-none absolute left-3 top-3 z-10 rounded border border-amber-500/40 bg-amber-50 px-2 py-1 text-[11px] text-amber-900">
            Overlay hidden — strokes still on this paper. Show on glass to paint other tabs.
          </div>
        )}
        {typeDraft && (
          <input
            autoFocus
            data-testid="draw-type-input"
            className="absolute z-20 min-w-[8rem] rounded border border-[color:var(--accent)] bg-white px-2 py-1 text-sm text-[color:var(--text)] shadow"
            style={{
              left: typeDraft.clientX,
              top: typeDraft.clientY,
              transform: "translate(-50%, -50%)",
              position: "fixed",
            }}
            value={typeDraft.text}
            onChange={(e) => setTypeDraft({ ...typeDraft, text: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                commitTypeDraft();
              } else if (e.key === "Escape") {
                e.preventDefault();
                setTypeDraft(null);
              }
            }}
            onBlur={() => commitTypeDraft()}
            placeholder="Type label…"
          />
        )}
        <svg
          ref={svgRef}
          className={`h-full w-full touch-none bg-white select-none ${mode === "type" ? "cursor-text" : "cursor-crosshair"}`}
          viewBox="0 0 1000 1000"
          preserveAspectRatio="xMidYMid meet"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerLeave={onPointerUp}
          data-testid="draw-canvas"
          style={{ WebkitUserSelect: "none", userSelect: "none" }}
        >
          <rect width="1000" height="1000" fill="#ffffff" />
          {Array.from({ length: 51 }, (_, i) => (
            <g key={`minor-${i}`} stroke="#f0f0f0" strokeWidth="0.75">
              <line x1={i * 20} y1={0} x2={i * 20} y2={1000} />
              <line x1={0} y1={i * 20} x2={1000} y2={i * 20} />
            </g>
          ))}
          {Array.from({ length: 11 }, (_, i) => (
            <g key={`major-${i}`} stroke="#e5e5e5" strokeWidth="1.25">
              <line x1={i * 100} y1={0} x2={i * 100} y2={1000} />
              <line x1={0} y1={i * 100} x2={1000} y2={i * 100} />
            </g>
          ))}
          {scene.strokes.map((s, i) => {
            if (s.type === "path") {
              return (
                <path
                  key={i}
                  d={pathToSvgD(s.points, s.closed)}
                  stroke={s.stroke ?? INK}
                  strokeWidth={s.strokeWidth ?? 3}
                  fill={s.fill ?? "none"}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              );
            }
            if (s.type === "circle") {
              return (
                <circle
                  key={i}
                  cx={s.cx * 1000}
                  cy={s.cy * 1000}
                  r={s.r * 1000}
                  stroke={s.stroke ?? INK}
                  strokeWidth={s.strokeWidth ?? 3}
                  fill={s.fill ?? "none"}
                />
              );
            }
            if (s.type === "image") {
              return (
                <image
                  key={i}
                  href={s.href}
                  x={s.x * 1000}
                  y={s.y * 1000}
                  width={s.w * 1000}
                  height={s.h * 1000}
                  preserveAspectRatio="xMidYMid meet"
                  opacity={s.opacity ?? 1}
                />
              );
            }
            if (s.type === "text") {
              return (
                <text
                  key={i}
                  x={s.x * 1000}
                  y={s.y * 1000}
                  fill={s.fill ?? INK}
                  fontSize={s.fontSize ?? 18}
                  textAnchor="middle"
                  fontFamily="Georgia, serif"
                  className="pointer-events-none select-none"
                  style={{ userSelect: "none" }}
                >
                  {s.text}
                </text>
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
                  stroke={s.stroke ?? INK}
                  strokeWidth={s.strokeWidth ?? 3}
                  strokeLinecap="round"
                />
              );
            }
            if (s.type === "ellipse") {
              return (
                <ellipse
                  key={i}
                  cx={s.cx * 1000}
                  cy={s.cy * 1000}
                  rx={s.rx * 1000}
                  ry={s.ry * 1000}
                  stroke={s.stroke ?? INK}
                  strokeWidth={s.strokeWidth ?? 3}
                  fill={s.fill ?? "none"}
                />
              );
            }
            return null;
          })}
          {pts.length >= 2 && (
            <path
              d={pathToSvgD(pts, false)}
              stroke={INK}
              strokeWidth={3}
              fill="none"
              strokeLinecap="round"
            />
          )}
        </svg>
      </div>
    </div>
  );
}
