/**
 * Time formatting for the UI.
 *
 * All wall-clock displays render in a single fixed timezone — US Eastern,
 * DST-aware (`America/New_York`) — so every viewer sees the same time
 * regardless of their machine's zone. Because the zone is pinned via `Intl`,
 * output is independent of the host/CI timezone, which also makes these
 * functions deterministic under test.
 */

/** The single timezone all wall-clock displays render in (DST-aware Eastern). */
export const APP_TIME_ZONE = "America/New_York";

const DAY_MS = 86_400_000;

/** Full datetime parts in Eastern, including the zone abbreviation (EDT/EST). */
const partsFmt = new Intl.DateTimeFormat("en-US", {
  timeZone: APP_TIME_ZONE,
  year: "numeric",
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
  timeZoneName: "short",
});

/** Eastern calendar day as a sortable `YYYY-MM-DD` key (en-CA yields ISO order). */
const dayKeyFmt = new Intl.DateTimeFormat("en-CA", {
  timeZone: APP_TIME_ZONE,
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});

interface EasternParts {
  year: string;
  month: string;
  day: string;
  hour: string;
  minute: string;
  second: string;
  tz: string;
}

function easternParts(d: Date): EasternParts {
  const parts = partsFmt.formatToParts(d);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  // Some ICU builds render midnight as "24" with hour12:false; normalize.
  const hour = get("hour") === "24" ? "00" : get("hour");
  return {
    year: get("year"),
    month: get("month"),
    day: get("day"),
    hour,
    minute: get("minute"),
    second: get("second"),
    tz: get("timeZoneName"),
  };
}

/** Eastern calendar day of an instant, as `YYYY-MM-DD`. Exported for day-grouping. */
export function easternDayKey(d: Date): string {
  return dayKeyFmt.format(d);
}

/** Format milliseconds as human-readable duration */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const secs = seconds % 60;
  if (minutes < 60) return `${minutes}m ${secs}s`;
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  return `${hours}h ${mins}m`;
}

/** Format ISO timestamp as relative time (e.g., "2m ago") */
export function relativeTime(iso: string): string {
  return relativeTimeFrom(iso, Date.now());
}

/** Format ISO timestamp as relative time from a given reference point */
export function relativeTimeFrom(iso: string, now: number): string {
  const diff = now - new Date(iso).getTime();
  if (diff < 0) return "just now";
  if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

/**
 * Compact Eastern timestamp for lists/rows: a smart date + HH:MM:SS + zone.
 * Today / Yesterday for recent days, "Mon DD" within the current year, and
 * "Mon DD YYYY" for older — e.g. "Today 21:53:59 EDT", "Jun 8 17:44:49 EDT",
 * "Dec 3 2025 09:12:00 EST".
 */
export function compactTime(iso: string): string {
  return compactTimeFrom(iso, Date.now());
}

/** `compactTime` with an explicit "now" reference (deterministic for tests). */
export function compactTimeFrom(iso: string, now: number): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const p = easternParts(d);
  const time = `${p.hour}:${p.minute}:${p.second}`;

  const key = easternDayKey(d);
  const todayKey = easternDayKey(new Date(now));
  const yesterdayKey = easternDayKey(new Date(now - DAY_MS));

  let datePart: string;
  if (key === todayKey) {
    datePart = "Today";
  } else if (key === yesterdayKey) {
    datePart = "Yesterday";
  } else {
    const nowYear = easternParts(new Date(now)).year;
    datePart =
      p.year === nowYear ? `${p.month} ${p.day}` : `${p.month} ${p.day} ${p.year}`;
  }

  return `${datePart} ${time} ${p.tz}`;
}

/**
 * Full absolute Eastern timestamp for detail panels — always shows the date
 * (no Today/Yesterday wording): "Jun 8 2026 21:53:59 EDT".
 */
export function formatTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const p = easternParts(d);
  return `${p.month} ${p.day} ${p.year} ${p.hour}:${p.minute}:${p.second} ${p.tz}`;
}
