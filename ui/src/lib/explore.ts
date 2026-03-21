/** Pure transforms for the Explore tab — session list grouping, search, sort, filter. */

import type { SessionSummary } from "@/types/session";

/** A group of sessions under a header (day or project). */
export interface SessionGroup {
  readonly label: string;
  readonly sessions: readonly SessionSummary[];
}

/** Status filter for the Explore sidebar. */
export type SessionStatusFilter = "all" | "ongoing" | "completed" | "errored" | "stale";

/** Filter sessions by status. */
export function filterSessionsByStatus(
  sessions: readonly SessionSummary[],
  status: SessionStatusFilter,
): SessionSummary[] {
  if (status === "all") return [...sessions];
  return sessions.filter((s) => s.status === status);
}

/** Filter sessions by project. Empty string = all projects. */
export function filterSessionsByProject(
  sessions: readonly SessionSummary[],
  projectId: string,
): SessionSummary[] {
  if (!projectId) return [...sessions];
  return sessions.filter((s) => (s.project_id ?? "") === projectId);
}

/** Extract unique projects from sessions, sorted by frequency. */
export function extractProjects(
  sessions: readonly SessionSummary[],
): { id: string; name: string; count: number }[] {
  const counts = new Map<string, { name: string; count: number }>();
  for (const s of sessions) {
    const id = s.project_id ?? "";
    if (!id) continue;
    const existing = counts.get(id);
    if (existing) {
      existing.count++;
    } else {
      counts.set(id, { name: s.project_name ?? id, count: 1 });
    }
  }
  return Array.from(counts.entries())
    .map(([id, { name, count }]) => ({ id, name, count }))
    .sort((a, b) => b.count - a.count);
}

/** Compute status counts from a list of sessions. */
export function computeStatusCounts(
  sessions: readonly SessionSummary[],
): Record<SessionStatusFilter, number> {
  const counts: Record<SessionStatusFilter, number> = {
    all: sessions.length,
    ongoing: 0,
    completed: 0,
    errored: 0,
    stale: 0,
  };
  for (const s of sessions) {
    if (s.status in counts) counts[s.status as SessionStatusFilter]++;
  }
  return counts;
}

/** Check if a session ID belongs to an agent (subagent spawn). */
export function isAgentSession(sessionId: string): boolean {
  return sessionId.startsWith("agent-");
}

/** A parent session with its agent children. */
export interface ParentSession {
  readonly session: SessionSummary;
  readonly agents: readonly SessionSummary[];
  readonly totalAgentEvents: number;
}

/** Build a hierarchy: main sessions as parents, agent sessions grouped underneath by project.
 *  Orphan agents (no matching main session in same project) become top-level entries.
 *  Sorted by most recent first. */
export function buildSessionHierarchy(sessions: readonly SessionSummary[]): ParentSession[] {
  const main: SessionSummary[] = [];
  const agents: SessionSummary[] = [];

  for (const s of sessions) {
    if (isAgentSession(s.session_id)) {
      agents.push(s);
    } else {
      main.push(s);
    }
  }

  // Index main sessions by project_id
  const mainByProject = new Map<string, SessionSummary>();
  for (const m of main) {
    const pid = m.project_id ?? "";
    if (pid) mainByProject.set(pid, m);
  }

  // Group agents under their parent by project
  const agentsByParent = new Map<string, SessionSummary[]>();
  const orphans: SessionSummary[] = [];

  for (const a of agents) {
    const pid = a.project_id ?? "";
    const parent = pid ? mainByProject.get(pid) : undefined;
    if (parent) {
      const list = agentsByParent.get(parent.session_id);
      if (list) list.push(a);
      else agentsByParent.set(parent.session_id, [a]);
    } else {
      orphans.push(a);
    }
  }

  // Build result
  const result: ParentSession[] = [];

  for (const m of main) {
    const children = agentsByParent.get(m.session_id) ?? [];
    const totalAgentEvents = children.reduce((sum, a) => sum + a.event_count, 0);
    result.push({ session: m, agents: children, totalAgentEvents });
  }

  // Orphan agents become their own top-level entries
  for (const o of orphans) {
    result.push({ session: o, agents: [], totalAgentEvents: 0 });
  }

  // Sort by most recent first
  result.sort((a, b) =>
    new Date(b.session.start_time).getTime() - new Date(a.session.start_time).getTime(),
  );

  return result;
}

/** Group sessions by day (e.g., "Today", "Yesterday", "2025-01-14"). */
export function groupSessionsByDay(
  sessions: readonly SessionSummary[],
  now: Date = new Date(),
): SessionGroup[] {
  const sorted = [...sessions].sort(
    (a, b) => new Date(b.start_time).getTime() - new Date(a.start_time).getTime(),
  );

  const groups = new Map<string, SessionSummary[]>();

  for (const s of sorted) {
    const label = dayLabel(s.start_time, now);
    const group = groups.get(label);
    if (group) group.push(s);
    else groups.set(label, [s]);
  }

  return Array.from(groups.entries()).map(([label, sessions]) => ({ label, sessions }));
}

/** Filter sessions by search query (matches against session_id, first_prompt, project_name). */
export function filterSessionsByQuery(
  sessions: readonly SessionSummary[],
  query: string,
): SessionSummary[] {
  if (!query.trim()) return [...sessions];
  const q = query.toLowerCase();
  return sessions.filter(
    (s) =>
      s.session_id.toLowerCase().includes(q) ||
      (s.first_prompt?.toLowerCase().includes(q) ?? false) ||
      (s.project_name?.toLowerCase().includes(q) ?? false),
  );
}

/** Derive a human-readable day label from an ISO timestamp. */
export function dayLabel(iso: string, now: Date = new Date()): string {
  const date = new Date(iso);
  const today = startOfDay(now);
  const target = startOfDay(date);

  const diffDays = Math.floor((today.getTime() - target.getTime()) / 86_400_000);

  if (diffDays === 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  if (diffDays < 7) return date.toLocaleDateString("en-US", { weekday: "long" });
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
}

function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}
