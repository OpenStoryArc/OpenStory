/**
 * Build an index mapping event IDs to the patterns they belong to.
 *
 * Pure function: patterns[] → Map<eventId, PatternView[]>
 */
import type { PatternView } from "@/types/wire-record";

export function buildPatternIndex(
  patterns: readonly PatternView[],
): ReadonlyMap<string, PatternView[]> {
  const index = new Map<string, PatternView[]>();
  for (const p of patterns) {
    for (const eventId of p.events) {
      const existing = index.get(eventId);
      if (existing) {
        existing.push(p);
      } else {
        index.set(eventId, [p]);
      }
    }
  }
  return index;
}
