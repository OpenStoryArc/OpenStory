/** Pure core of the resizable side panel. Kept separate from the React hook so
 *  the bounds logic is testable without a DOM. A dragged width is ALWAYS clamped
 *  to [min, max] so the panel can neither collapse to nothing nor swallow the
 *  view; a non-finite width (a drag glitch) falls back to min. */
export function clampWidth(px: number, min: number, max: number): number {
  if (!Number.isFinite(px)) return px > 0 ? max : min;
  return Math.min(max, Math.max(min, px));
}
