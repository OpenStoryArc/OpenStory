/**
 * Draw tab — free canvas for human doodling + agent pen demos.
 * Agent strokes land here too via the global draw$ scene.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { clearDraw, commitDraw, drawScene$ } from "@/streams/draw";
import { smileyStrokes, type DrawScene, type NormPoint } from "@/lib/draw";

export function DrawView() {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const [scene, setScene] = useState<DrawScene>({ strokes: [], visible: true });
  const [drawing, setDrawing] = useState(false);
  const [pts, setPts] = useState<NormPoint[]>([]);
  const color = "#2f4a3e";

  useEffect(() => {
    const sub = drawScene$().subscribe(setScene);
    return () => sub.unsubscribe();
  }, []);

  const toNorm = useCallback((clientX: number, clientY: number): NormPoint | null => {
    const el = svgRef.current;
    if (!el) return null;
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return null;
    return {
      x: Math.min(1, Math.max(0, (clientX - r.left) / r.width)),
      y: Math.min(1, Math.max(0, (clientY - r.top) / r.height)),
    };
  }, []);

  const onPointerDown = (e: React.PointerEvent) => {
    const p = toNorm(e.clientX, e.clientY);
    if (!p) return;
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
        strokes: [{ type: "path", points: pts, stroke: color, strokeWidth: 3 }],
        label: "human",
      });
    }
    setPts([]);
  };

  return (
    <div className="flex flex-1 min-h-0 flex-col" data-testid="draw-view">
      <div className="flex flex-wrap items-center gap-2 border-b border-[color:var(--border)] bg-[color:var(--bg-surface)] px-4 py-2">
        <p className="text-sm text-[color:var(--text-muted)]">
          Agent pen · ui.* ink only — never history. Draw with the pointer, or let an agent drive.
        </p>
        <div className="ml-auto flex gap-2">
          <button
            type="button"
            className="rounded border border-[color:var(--border)] px-2 py-1 text-xs hover:border-[color:var(--accent)]"
            onClick={() =>
              commitDraw({
                clear: true,
                strokes: smileyStrokes(),
                label: "smiley",
              })
            }
          >
            Smiley
          </button>
          <button
            type="button"
            className="rounded border border-[color:var(--border)] px-2 py-1 text-xs hover:border-[color:var(--accent)]"
            onClick={() => {
              void (async () => {
                const { portraitInkStrokes, geometricMaxStrokes } = await import(
                  "@/lib/draw-portrait"
                );
                try {
                  const strokes = await portraitInkStrokes(
                    "https://github.com/maxglassie.png",
                    { caption: "Max Glassie" },
                  );
                  commitDraw({ clear: true, strokes, label: "edge-ink Max" });
                } catch {
                  commitDraw({
                    clear: true,
                    strokes: geometricMaxStrokes(),
                    label: "geometric Max",
                  });
                }
              })();
            }}
          >
            Draw Max (ink)
          </button>
          <button
            type="button"
            className="rounded border border-[color:var(--border)] px-2 py-1 text-xs hover:border-[color:var(--accent)]"
            onClick={() => {
              void import("@/lib/draw-portrait").then(({ geometricMaxStrokes }) => {
                commitDraw({
                  clear: true,
                  strokes: geometricMaxStrokes(),
                  label: "geometric Max",
                });
              });
            }}
          >
            Geometric Max
          </button>
          <button
            type="button"
            className="rounded border border-[color:var(--border)] px-2 py-1 text-xs hover:border-[color:var(--accent)]"
            onClick={() => clearDraw()}
          >
            Clear
          </button>
        </div>
      </div>
      <div className="relative flex-1 min-h-0 bg-[color:var(--bg)]">
        <svg
          ref={svgRef}
          className="h-full w-full touch-none cursor-crosshair"
          viewBox="0 0 1000 1000"
          preserveAspectRatio="none"
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerLeave={onPointerUp}
          data-testid="draw-canvas"
        >
          <rect width="1000" height="1000" fill="var(--bg, #faf9f7)" />
          {/* faint grid */}
          {Array.from({ length: 10 }, (_, i) => (
            <g key={i} stroke="#e7e5e4" strokeWidth="1">
              <line x1={i * 100} y1={0} x2={i * 100} y2={1000} />
              <line x1={0} y1={i * 100} x2={1000} y2={i * 100} />
            </g>
          ))}
          {scene.strokes.map((s, i) => {
            if (s.type === "path") {
              const d = s.points
                .map((p, j) => `${j === 0 ? "M" : "L"} ${(p.x * 1000).toFixed(1)} ${(p.y * 1000).toFixed(1)}`)
                .join(" ");
              return (
                <path
                  key={i}
                  d={d}
                  stroke={s.stroke ?? color}
                  strokeWidth={s.strokeWidth ?? 3}
                  fill="none"
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
                  stroke={s.stroke ?? color}
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
                />
              );
            }
            if (s.type === "text") {
              return (
                <text
                  key={i}
                  x={s.x * 1000}
                  y={s.y * 1000}
                  fill={s.fill ?? color}
                  fontSize={s.fontSize ?? 18}
                  textAnchor="middle"
                  fontFamily="Georgia, serif"
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
                  stroke={s.stroke ?? color}
                  strokeWidth={s.strokeWidth ?? 3}
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
                  stroke={s.stroke ?? color}
                  strokeWidth={s.strokeWidth ?? 3}
                  fill={s.fill ?? "none"}
                />
              );
            }
            return null;
          })}
          {pts.length >= 2 && (
            <path
              d={pts
                .map((p, j) => `${j === 0 ? "M" : "L"} ${(p.x * 1000).toFixed(1)} ${(p.y * 1000).toFixed(1)}`)
                .join(" ")}
              stroke={color}
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
