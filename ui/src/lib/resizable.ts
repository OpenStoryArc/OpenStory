/** Pure core of the resizable side panel. Kept separate from the React hook so
 *  the bounds logic is testable without a DOM. A dragged width is ALWAYS clamped
 *  to [min, max] so the panel can neither collapse to nothing nor swallow the
 *  view; a non-finite width (a drag glitch) falls back to min. */
export function clampWidth(px: number, min: number, max: number): number {
  if (!Number.isFinite(px)) return px > 0 ? max : min;
  return Math.min(max, Math.max(min, px));
}

/** The direction a drag changes width, by which SIDE of the screen the panel
 *  sits on. A right-side panel is dragged by its LEFT edge (pointer left ⟶
 *  wider); a left-side sidebar is dragged by its RIGHT edge (pointer right ⟶
 *  wider). Pure; clamping is the caller's job. */
export function dragWidth(
  startWidth: number,
  startX: number,
  currentX: number,
  side: "left" | "right",
): number {
  const delta = currentX - startX;
  return side === "left" ? startWidth + delta : startWidth - delta;
}
