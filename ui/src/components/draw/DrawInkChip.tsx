/**
 * Floating attention-canvas chip on non-Draw tabs.
 * Shows the CURRENT CONTEXT's glass ink (routeGlassKey → glass-ink store),
 * mirroring DrawOverlay's scoping — same context, same count, same Clear.
 * Renders nothing wherever the overlay has no surface to record onto
 * (Draw tab, reels player) so it never offers Annotate it can't back up.
 */

import { useEffect, useState } from "react";
import { drawInteractive$, setDrawInteractive } from "@/streams/draw";
import { clearGlassContext, glassInkStore$ } from "@/streams/glass-ink";
import { emptyGlassInkStore, routeGlassKey, type GlassInkStore } from "@/lib/glass-ink";
import type { HashRoute } from "@/lib/hash-route";

export function DrawInkChip({
  route,
  onOpenDraw,
}: {
  readonly route: HashRoute;
  readonly onOpenDraw: () => void;
}) {
  const [store, setStore] = useState<GlassInkStore>(emptyGlassInkStore);
  const [interactive, setInteractive] = useState(false);

  useEffect(() => {
    const a = glassInkStore$().subscribe(setStore);
    const b = drawInteractive$().subscribe(setInteractive);
    return () => {
      a.unsubscribe();
      b.unsubscribe();
    };
  }, []);

  const glassKey = routeGlassKey(route);
  // No context owns this glass here (Draw tab, reels player) — never offer
  // Annotate on a surface DrawOverlay won't record onto.
  if (glassKey == null) return null;

  const n = store.byKey[glassKey]?.strokes.length ?? 0;

  return (
    <div
      className="pointer-events-auto fixed bottom-20 right-4 z-[120] flex max-w-[min(100vw-2rem,24rem)] flex-wrap items-center gap-1.5 rounded-full border border-[color:var(--border)] bg-[color:var(--bg-surface)]/95 px-2.5 py-1.5 text-[11px] text-[color:var(--text)] shadow-card backdrop-blur-sm"
      data-testid="draw-ink-chip"
      role="status"
      aria-label={
        interactive ? `Annotating, ${n} strokes here` : n > 0 ? `Attention canvas, ${n} strokes here` : "Attention canvas"
      }
    >
      <span className="px-1 font-medium text-[color:var(--accent)]" title={glassKey}>
        ✎ {n} here
        {interactive ? " · annotate" : ""}
      </span>
      <button
        type="button"
        className={`rounded-full border px-2 py-0.5 ${
          interactive
            ? "border-[color:var(--accent)] bg-[color:var(--accent)]/15 text-[color:var(--accent)]"
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
        Board
      </button>
      {n > 0 && (
        <button
          type="button"
          className="rounded-full border border-[color:var(--border)] px-2 py-0.5 hover:border-[color:var(--accent)]"
          onClick={() => clearGlassContext(glassKey)}
          title="Clear ink on this view"
        >
          Clear
        </button>
      )}
    </div>
  );
}
