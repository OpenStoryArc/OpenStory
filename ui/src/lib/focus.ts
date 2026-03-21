/** Focus utilities for the timeline.
 *
 * Pure functions for focus state management — no React dependencies. */

/** Returns true if the focus should be cleared because the focused event
 *  is no longer in the visible set (e.g., filtered out). */
export function shouldClearFocus(
  focusRootId: string | null,
  visibleIds: ReadonlySet<string>,
): boolean {
  if (focusRootId === null) return false;
  return !visibleIds.has(focusRootId);
}
