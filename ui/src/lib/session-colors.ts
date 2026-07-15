/**
 * Deterministic session color assignment — theme-aware.
 *
 * Same `session_id` → same color slot, every time, across every component
 * that displays it. Colors are CSS variables (`--sc-0`…`--sc-9`, defined for
 * both themes in index.css): pastels on dark, darker same-hue inks on light —
 * so session chips stay legible when the theme flips, live, with no rerender.
 *
 * Because the value is a `var()` reference (not a hex), NEVER concatenate
 * alpha suffixes onto it (`${fg}18` breaks). Use `sessionTint()` for
 * backgrounds/borders — it wraps the color in a theme-correct `color-mix`.
 */

const SLOT_COUNT = 10;

/** Pick a deterministic color slot for a session_id (djb2-style hash). */
function slot(sessionId: string): number {
  let hash = 0;
  for (let i = 0; i < sessionId.length; i++) {
    hash = ((hash << 5) - hash + sessionId.charCodeAt(i)) | 0;
  }
  return Math.abs(hash) % SLOT_COUNT;
}

/** Foreground/text color for a session — a CSS var() reference. */
export function sessionColor(sessionId: string): string {
  return `var(--sc-${slot(sessionId)})`;
}

/** Translucent wash of the session color, for chip backgrounds/borders.
 *  `percent` ≈ the old hex-alpha conventions (18→9, 33→20, etc.). */
export function sessionTint(sessionId: string, percent: number): string {
  return `color-mix(in oklab, var(--sc-${slot(sessionId)}) ${percent}%, transparent)`;
}

/** Tint an arbitrary color string (hex or var()) — for components that
 *  receive a color prop rather than a session id. */
export function tint(color: string, percent: number): string {
  return `color-mix(in oklab, ${color} ${percent}%, transparent)`;
}

/**
 * Convenience helper: the {fg, bg, border} triple a chip needs.
 */
export function sessionChipStyle(sessionId: string): {
  readonly fg: string;
  readonly bg: string;
  readonly border: string;
} {
  return {
    fg: sessionColor(sessionId),
    bg: sessionTint(sessionId, 13),
    border: sessionTint(sessionId, 26),
  };
}
