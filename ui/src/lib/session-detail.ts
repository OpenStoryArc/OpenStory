/** Pure transforms for Session Detail Panel data. */

// ── Types matching backend API responses ──

export interface SessionSynopsis {
  readonly session_id: string;
  readonly label: string | null;
  readonly project_id: string | null;
  readonly project_name: string | null;
  readonly event_count: number;
  readonly tool_count: number;
  readonly error_count: number;
  readonly first_event: string | null;
  readonly last_event: string | null;
  readonly duration_secs: number | null;
  readonly top_tools: readonly ToolCount[];
}

export interface ToolCount {
  readonly tool: string;
  readonly count: number;
}

export interface FileImpact {
  readonly file: string;
  readonly reads: number;
  readonly writes: number;
}

export interface SessionError {
  readonly timestamp: string;
  readonly message: string;
}

// ── Derived metrics ──

export interface SynopsisMetrics {
  readonly events: number;
  readonly tools: number;
  readonly errors: number;
  readonly duration: string;
}

// ── Transforms ──

/** Derive display metrics from a synopsis. */
export function deriveSynopsisMetrics(s: SessionSynopsis): SynopsisMetrics {
  return {
    events: s.event_count,
    tools: s.tool_count,
    errors: s.error_count,
    duration: formatSynopsisDuration(s.duration_secs),
  };
}

/** Format duration in seconds to compact display string. */
export function formatSynopsisDuration(secs: number | null): string {
  if (secs == null || secs <= 0) return "—";
  if (secs < 60) return `${secs}s`;
  const minutes = Math.floor(secs / 60);
  const remainder = secs % 60;
  if (minutes < 60) return remainder > 0 ? `${minutes}m ${remainder}s` : `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
}

/** Sort file impact entries by total operations (reads + writes), descending. */
export function sortFileImpact(files: readonly FileImpact[]): FileImpact[] {
  return [...files].sort((a, b) => {
    const totalDiff = (b.reads + b.writes) - (a.reads + a.writes);
    if (totalDiff !== 0) return totalDiff;
    return a.file.localeCompare(b.file);
  });
}

/** Extract the basename from a file path. */
export function fileBasename(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] ?? path;
}

/** Truncate an error message to a max length. */
export function truncateError(msg: string, max: number = 200): string {
  if (msg.length <= max) return msg;
  return msg.slice(0, max) + "…";
}
