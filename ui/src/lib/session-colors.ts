/**
 * Deterministic session color assignment.
 *
 * Same `session_id` → same color, every time, across every component that
 * displays it. The color set is the Tokyo Night palette + a few neighbours,
 * picked to be visually distinct against the dark background.
 *
 * Use the base `sessionColor()` for foreground/text colors. For background
 * tints and borders, suffix the hex with the alpha hex codes the rest of the
 * UI uses by convention:
 *
 *   const fg = sessionColor(sid);
 *   <span style={{ color: fg, background: `${fg}18`, border: `1px solid ${fg}33` }}>
 *
 * The `18` and `33` suffixes correspond to ~9% and ~20% alpha, matching the
 * existing chip styling in TurnCard / Sidebar.
 */

const SESSION_COLORS = [
  "#7aa2f7", // blue
  "#bb9af7", // purple
  "#2ac3de", // bright cyan
  "#9ece6a", // green
  "#e0af68", // yellow
  "#f7768e", // pink
  "#7dcfff", // cyan
  "#ff9e64", // orange
  "#c0caf5", // light gray
  "#73daca", // teal
] as const;

/**
 * Pick a deterministic color from the palette for a given session_id.
 * Uses a simple djb2-style hash so same input always maps to same color.
 */
export function sessionColor(sessionId: string): string {
  let hash = 0;
  for (let i = 0; i < sessionId.length; i++) {
    hash = ((hash << 5) - hash + sessionId.charCodeAt(i)) | 0;
  }
  return SESSION_COLORS[Math.abs(hash) % SESSION_COLORS.length]!;
}

/**
 * Convenience helper: return the {fg, bg, border} triple a chip needs.
 * Avoids string concatenation at every callsite.
 */
export function sessionChipStyle(sessionId: string): {
  readonly fg: string;
  readonly bg: string;
  readonly border: string;
} {
  const fg = sessionColor(sessionId);
  return {
    fg,
    bg: `${fg}18`,
    border: `${fg}33`,
  };
}
