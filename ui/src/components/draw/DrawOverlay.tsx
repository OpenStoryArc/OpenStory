/**
 * Full-viewport SVG ink layer for agent (and human) drawing.
 * pointer-events: none so the mirror stays usable underneath —
 * Draw tab opts into interactive drawing separately.
 */

import { useEffect, useState } from "react";
import { drawScene$ } from "@/streams/draw";
import { pathToSvgD, type DrawScene, type DrawStroke } from "@/lib/draw";

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

export function DrawOverlay() {
  const [scene, setScene] = useState<DrawScene>({ strokes: [], visible: true });

  useEffect(() => {
    const sub = drawScene$().subscribe(setScene);
    return () => sub.unsubscribe();
  }, []);

  if (!scene.visible || scene.strokes.length === 0) return null;

  return (
    <div
      className="pointer-events-none fixed inset-0 z-[80]"
      data-testid="draw-overlay"
      aria-hidden
    >
      {scene.label && (
        <div className="pointer-events-none absolute right-3 top-14 z-[81] rounded-full border border-[color:var(--accent)]/40 bg-[color:var(--bg-surface)]/90 px-2.5 py-1 text-[11px] text-[color:var(--accent)] shadow-card">
          ✎ {scene.label}
        </div>
      )}
      <svg
        className="h-full w-full"
        viewBox="0 0 1000 1000"
        preserveAspectRatio="none"
      >
        {scene.strokes.map((s, i) => (
          <StrokeEl key={i} s={s} />
        ))}
      </svg>
    </div>
  );
}
