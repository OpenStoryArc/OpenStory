import type { SessionSummary } from "@/types/session";

export type StatusFilter = "all" | "ongoing" | "completed" | "errored" | "stale";

export function filterSessions(
  sessions: readonly SessionSummary[],
  filter: StatusFilter,
): readonly SessionSummary[] {
  if (filter === "all") return sessions;
  return sessions.filter((s) => s.status === filter);
}

/** Group sessions by project_id. Sessions without a project_id go into "unknown". */
export function groupByProject(
  sessions: readonly SessionSummary[],
): Map<string, SessionSummary[]> {
  const groups = new Map<string, SessionSummary[]>();
  for (const s of sessions) {
    const key = s.project_id ?? "unknown";
    const group = groups.get(key);
    if (group) {
      group.push(s);
    } else {
      groups.set(key, [s]);
    }
  }
  return groups;
}

/** Derive a human-readable project display name.
 *  Prefers the API-provided project_name, falls back to project_id decoding. */
export function projectDisplayName(
  projectId: string,
  sessions: readonly SessionSummary[],
): string {
  if (projectId === "unknown") return "Unknown Project";
  // Prefer project_name from API (resolved by backend against watch_dir entries)
  for (const s of sessions) {
    if (s.project_name) return s.project_name;
  }
  // Fallback: decode project_id (e.g., "-Users-max-projects-foo" → "foo")
  const parts = projectId.split("-").filter(Boolean);
  const lastPart = parts[parts.length - 1];
  return lastPart ?? projectId;
}
