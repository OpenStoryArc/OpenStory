/** Subtree membership — pure functions for tree-based filtering.
 *
 *  Given a "focus root" event ID, determines which events are descendants.
 *  Uses parent-chain walking with path compression for O(n) total work. */

/** Minimal shape needed from events — works with both ViewRecord and WireRecord. */
interface HasTreeFields {
  readonly id: string;
  readonly parent_uuid: string | null;
}

/** Build a parent lookup from an array of events.
 *  Maps each event ID to its parent_uuid (or null for roots). */
export function buildParentIndex(
  events: readonly HasTreeFields[],
): ReadonlyMap<string, string | null> {
  const index = new Map<string, string | null>();
  for (const ev of events) {
    index.set(ev.id, ev.parent_uuid);
  }
  return index;
}

/** Given a focus root and a parent index, return the set of all
 *  descendant IDs (including the root itself).
 *
 *  Walks each event's parent chain upward. Uses path compression:
 *  once an event is confirmed in/out of the subtree, all events on
 *  its chain are cached. O(n) total regardless of depth. */
export function subtreeIds(
  focusRootId: string,
  parentIndex: ReadonlyMap<string, string | null>,
): ReadonlySet<string> {
  const inSubtree = new Set<string>([focusRootId]);
  const notInSubtree = new Set<string>();

  for (const eventId of parentIndex.keys()) {
    if (inSubtree.has(eventId) || notInSubtree.has(eventId)) continue;

    // Walk the parent chain, collecting the path
    const path: string[] = [eventId];
    let current: string | null = parentIndex.get(eventId) ?? null;

    while (current !== null) {
      if (inSubtree.has(current)) {
        // Found the focus root (or a known descendant) — entire path is in
        for (const id of path) inSubtree.add(id);
        break;
      }
      if (notInSubtree.has(current)) {
        // Hit a known non-member — entire path is out
        for (const id of path) notInSubtree.add(id);
        break;
      }
      path.push(current);
      current = parentIndex.get(current) ?? null;
    }

    // If we walked to null without hitting a known node, the path is outside
    if (current === null && !inSubtree.has(path[path.length - 1]!)) {
      for (const id of path) notInSubtree.add(id);
    }
  }

  return inSubtree;
}
