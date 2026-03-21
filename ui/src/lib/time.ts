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
