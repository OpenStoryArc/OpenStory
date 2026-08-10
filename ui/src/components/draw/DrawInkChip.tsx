/**
 * Floating attention-canvas chip on non-Draw tabs.
 * Always available (even with empty ink) so Annotate works on reels slides.
 */

import { useEffect, useState } from "react";
import {
  clearDraw,
  drawInteractive$,
  drawScene$,
  setDrawInteractive,
  setDrawVisible,
} from "@/streams/draw";
import type { DrawScene } from "@/lib/draw";

export function DrawInkChip({
  onOpenDraw,
}: {
  readonly onOpenDraw: () => void;
}) {
  const [scene, setScene] = useState<DrawScene>({ strokes: [], visible: true });
  const [interactive, setInteractive] = useState(false);

  useEffect(() => {
    const a = drawScene$().subscribe(setScene);
    const b = drawInteractive$().subscribe(setInteractive);
    return () => {
      a.unsubscribe();
      b.unsubscribe();
    };
  }, []);

  const n = scene.strokes.length;
  const hidden = scene.visible === false;

  return (
    <div
      className="pointer-events-auto fixed bottom-20 right-4 z-[120] flex max-w-[min(100vw-2rem,24rem)] flex-wrap items-center gap-1.5 rounded-full border border-[color:var(--border)] bg-[color:var(--bg-surface)]/95 px-2.5 py-1.5 text-[11px] text-[color:var(--text)] shadow-card backdrop-blur-sm"
      data-testid="draw-ink-chip"
      role="status"
      aria-label={
        interactive
          ? `Annotating, ${n} strokes`
          : n > 0
            ? `Attention canvas, ${n} strokes`
            : "Attention canvas"
      }
    >
      <span className="px-1 font-medium text-[color:var(--accent)]" title={scene.label ?? "ink"}>
        ✎ {n} stroke{n === 1 ? "" : "s"}
        {hidden ? " · hidden" : ""}
        {interactive ? " · annotate" : ""}
      </span>
      <button
        type="button"
        className={`rounded-full border px-2 py-0.5 ${
          interactive
            ? "border-rose-500 bg-rose-500/20 text-rose-700 dark:text-rose-200"
            : "border-[color:var(--border)] hover:border-[color:var(--accent)]"
        }`}
        onClick={() => setDrawInteractive(!interactive)}
        title={
          interactive
            ? "Stop annotating — clicks go through to reels/UI"
            : "Draw on the glass over this view (reels slides, story, …)"
        }
        data-testid="draw-annotate-toggle"
      >
        {interactive ? "Done" : "Annotate"}
      </button>
      <button
        type="button"
        className="rounded-full border border-[color:var(--border)] px-2 py-0.5 hover:border-[color:var(--accent)]"
        onClick={onOpenDraw}
      >
        Draw
      </button>
      {n > 0 && (
        <>
          <button
            type="button"
            className="rounded-full border border-[color:var(--border)] px-2 py-0.5 hover:border-[color:var(--accent)]"
            onClick={() => setDrawVisible(hidden)}
            title={hidden ? "Show ink on the glass" : "Hide ink without clearing"}
          >
            {hidden ? "Show" : "Hide"}
          </button>
          <button
            type="button"
            className="rounded-full border border-[color:var(--border)] px-2 py-0.5 hover:border-[color:var(--accent)]"
            onClick={() => clearDraw()}
            title="Clear all attention ink"
          >
            Clear
          </button>
        </>
      )}
    </div>
  );
}
