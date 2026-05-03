/**
 * Time-window filter — pure helpers + the discriminated type.
 *
 * Used by the Live sidebar's TimeFilter component to scope the
 * visible session list to recent activity. Filters compose with the
 * existing user/host filters (logical AND).
 */

export type TimeFilterKey = "1h" | "today" | "week" | "all";

/** Display labels — surfaced as button text in `<TimeFilter>`. */
export const TIME_FILTER_LABELS: Record<TimeFilterKey, string> = {
  "1h": "Last Hour",
  today: "Today",
  week: "This Week",
  all: "All",
};

/** The order chips appear in the row, from narrowest → widest. */
export const TIME_FILTER_ORDER: readonly TimeFilterKey[] = [
  "1h",
  "today",
  "week",
  "all",
];

/**
 * Returns the lower-bound timestamp (ms epoch) for a given filter at a
 * given "now". Returns 0 for `all` so any timestamp comparison passes.
 *
 * "today" = midnight local time. "week" = midnight on the most recent
 * Sunday in local time (matches the conventional calendar-week start in
 * en-US locales; if i18n becomes a concern we can lift this).
 */
export function timeFilterLowerBound(filter: TimeFilterKey, now: number): number {
  if (filter === "all") return 0;
  if (filter === "1h") return now - 60 * 60 * 1000;
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  if (filter === "today") return d.getTime();
  // week: roll back to most recent Sunday (getDay === 0).
  d.setDate(d.getDate() - d.getDay());
  return d.getTime();
}

/**
 * Pure predicate. Returns true when `timestamp` is within the active
 * window. Empty / unparseable timestamps fail every filter except
 * "all" — a session whose latest activity we can't parse shouldn't
 * leak into a "Last Hour" view.
 */
export function timeFilterMatches(
  timestamp: string,
  filter: TimeFilterKey,
  now: number = Date.now(),
): boolean {
  if (filter === "all") return true;
  if (!timestamp) return false;
  const ts = new Date(timestamp).getTime();
  if (Number.isNaN(ts)) return false;
  return ts >= timeFilterLowerBound(filter, now);
}
