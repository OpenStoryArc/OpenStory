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

// Theme-aware identity vars — same slots as sessionColor() (--sc-0..9 in
// index.css): pastels on dark, calligrapher inks on light.
const PERSON_COLORS = [
  "var(--sc-0)", "var(--sc-1)", "var(--sc-2)", "var(--sc-3)", "var(--sc-4)",
  "var(--sc-5)", "var(--sc-6)", "var(--sc-7)", "var(--sc-8)", "var(--sc-9)",
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
