/** Filter utilities for the Explore event timeline.
 *  Reuses TIMELINE_FILTERS predicates — same logic, different context. */

import type { WireRecord } from "@/types/wire-record";
import { TIMELINE_FILTERS } from "@/lib/timeline-filters";

/** Compute filter counts from a set of records. */
export function computeExploreCounts(records: readonly WireRecord[]): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const [name, predicate] of Object.entries(TIMELINE_FILTERS)) {
    if (name === "all") continue;
    let count = 0;
    for (const r of records) {
      if (predicate(r)) count++;
    }
    counts[name] = count;
  }
  return counts;
}

/** Record types that are noise — not useful in the Explore event list. */
const NOISE_TYPES = new Set(["token_usage", "file_snapshot", "session_meta", "context_compaction"]);

/** Filter out noise records (token_usage, file_snapshot, etc.). */
export function filterNoise(records: readonly WireRecord[]): WireRecord[] {
  return records.filter((r) => !NOISE_TYPES.has(r.record_type));
}

/** Apply a named filter to records. */
export function applyExploreFilter(
  records: readonly WireRecord[],
  filterName: string,
): WireRecord[] {
  const predicate = TIMELINE_FILTERS[filterName] ?? TIMELINE_FILTERS["all"]!;
  if (filterName === "all") return [...records];
  return records.filter((r) => predicate(r));
}
