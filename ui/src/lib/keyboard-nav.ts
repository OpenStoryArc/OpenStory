/**
 * Pure keyboard navigation logic for the Live Timeline.
 *
 * Computes the next selected index given arrow direction,
 * skipping turn divider rows.
 */

export function nextCardIndex(
  rows: readonly { category: string }[],
  currentIndex: number | null,
  direction: "up" | "down",
): number | null {
  if (rows.length === 0) return null;

  const step = direction === "down" ? 1 : -1;

  // No selection yet — find the first/last non-turn row
  if (currentIndex === null) {
    const start = direction === "down" ? 0 : rows.length - 1;
    for (let i = start; i >= 0 && i < rows.length; i += step) {
      if (rows[i]!.category !== "turn") return i;
    }
    return null;
  }

  // Move from current position, skipping turns
  for (let i = currentIndex + step; i >= 0 && i < rows.length; i += step) {
    if (rows[i]!.category !== "turn") return i;
  }

  // No valid target — stay put
  return currentIndex;
}
