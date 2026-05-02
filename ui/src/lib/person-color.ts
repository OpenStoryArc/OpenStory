/**
 * Deterministic person color assignment.
 *
 * Sibling of `sessionColor()` — same hash function, same Tokyo Night palette,
 * indexed by the user identifier instead of a session id. Every component
 * that renders a person gets the same color for the same name.
 *
 * Person colors and session colors come from the **same palette** by design:
 * a person's chip in the Live sidebar shares a hue with that person's
 * sessions, so the eye can connect them without a legend.
 */

const PERSON_COLORS = [
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

/** Stable color for a user/person identifier. */
export function personColor(user: string): string {
  let hash = 0;
  for (let i = 0; i < user.length; i++) {
    hash = (hash << 5) - hash + user.charCodeAt(i);
    hash |= 0;
  }
  const idx = Math.abs(hash) % PERSON_COLORS.length;
  return PERSON_COLORS[idx]!;
}
