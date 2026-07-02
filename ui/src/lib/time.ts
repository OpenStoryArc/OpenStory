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

/** Format ISO timestamp as local time string */
export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString();
}

/** Format ISO timestamp as compact time (HH:MM:SS) */
export function compactTime(iso: string): string {
  return new Date(iso).toTimeString().slice(0, 8);
}

/** Full local date + time — the WHEN in absolute form, for a hover title next
 *  to a relative time ("43d ago" ⟶ hover ⟶ "Jul 2, 2026, 8:38:25 AM"). Falls
 *  back to the raw input if unparseable so a card never renders "Invalid Date".*/
export function absoluteTime(iso: string): string {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? iso : d.toLocaleString();
}

/** The canonical absolute stamp used everywhere the date matters:
 *  `HH:MM:SS TZ YYYY/MM/DD` (24-hour, timezone abbreviation, then date) —
 *  e.g. "14:27:21 PDT 2026/07/02". Time-first because that's the scan target;
 *  the date follows so a bare time never hides the day. `opts.timeZone` (IANA)
 *  forces a zone — used by tests for determinism; omitted = the viewer's local
 *  zone. Invalid/empty input renders an em-dash, never "Invalid Date". */
export function fullTimestamp(iso: string, opts?: { timeZone?: string }): string {
  const d = new Date(iso);
  if (!iso || isNaN(d.getTime())) return "—";
  const fmt = new Intl.DateTimeFormat("en-GB", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    timeZoneName: "short",
    ...(opts?.timeZone ? { timeZone: opts.timeZone } : {}),
  });
  const parts = fmt.formatToParts(d);
  const get = (t: string) => parts.find((p) => p.type === t)?.value ?? "";
  // hour12:false can render "24" at midnight in some engines — normalize to "00".
  const hh = get("hour") === "24" ? "00" : get("hour");
  return `${hh}:${get("minute")}:${get("second")} ${get("timeZoneName")} ${get("year")}/${get("month")}/${get("day")}`;
}
