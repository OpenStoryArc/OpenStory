/** Pure aggregation layer for the Sessions Overview dashboard.
 *
 *  Turns the flat `GET /api/sessions` list into the models the dashboard
 *  renders: a per-day calendar grid, facet counts, filtered + sorted lists.
 *  All side-effect-free so it's exhaustively unit-testable and the calendar /
 *  filter math never leaks into the React/D3 boundary.
 */

import type { StorySession } from "@/lib/story-api";

// ── time helpers ──────────────────────────────────────────────────────────

/** Local `YYYY-MM-DD` for a Date. Calendar buckets are day-granular. */
export function dayKey(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Day bucket key for a session, from its start_time. `null` if unparseable. */
export function sessionDayKey(s: StorySession): string | null {
  if (!s.start_time) return null;
  const t = Date.parse(s.start_time);
  if (!Number.isFinite(t)) return null;
  return dayKey(new Date(t));
}

/** Session wall-clock duration in ms (last_event − start_time), floored at 0. */
export function sessionDurationMs(s: StorySession): number {
  if (!s.start_time || !s.last_event) return 0;
  const a = Date.parse(s.start_time);
  const b = Date.parse(s.last_event);
  if (!Number.isFinite(a) || !Number.isFinite(b)) return 0;
  return Math.max(0, b - a);
}

/** Total tokens (in + out) for a session. */
export function sessionTokens(s: StorySession): number {
  return (s.total_input_tokens ?? 0) + (s.total_output_tokens ?? 0);
}

/** Display key for a session's project — name, else id, else "unknown". */
export function projectKey(s: StorySession): string {
  return s.project_name || s.project_id || "unknown";
}

// ── day buckets ───────────────────────────────────────────────────────────

export interface DayBucket {
  readonly date: string; // YYYY-MM-DD
  readonly sessionCount: number;
  readonly eventCount: number;
  readonly tokens: number;
  readonly sessionIds: string[];
}

export function bucketByDay(sessions: readonly StorySession[]): Map<string, DayBucket> {
  const buckets = new Map<string, DayBucket>();
  for (const s of sessions) {
    const key = sessionDayKey(s);
    if (!key) continue;
    const existing = buckets.get(key);
    if (existing) {
      (existing.sessionIds as string[]).push(s.session_id);
      buckets.set(key, {
        date: key,
        sessionCount: existing.sessionCount + 1,
        eventCount: existing.eventCount + (s.event_count ?? 0),
        tokens: existing.tokens + sessionTokens(s),
        sessionIds: existing.sessionIds,
      });
    } else {
      buckets.set(key, {
        date: key,
        sessionCount: 1,
        eventCount: s.event_count ?? 0,
        tokens: sessionTokens(s),
        sessionIds: [s.session_id],
      });
    }
  }
  return buckets;
}

// ── calendar grid (GitHub contribution style) ─────────────────────────────

export interface CalendarCell {
  readonly date: string; // YYYY-MM-DD
  readonly weekIndex: number; // column (0 = leftmost/oldest)
  readonly dow: number; // 0 = Sunday … 6 = Saturday (row)
  readonly sessionCount: number;
  readonly eventCount: number;
  readonly tokens: number;
  /** 0 = empty, 1-4 = increasing activity intensity. */
  readonly level: 0 | 1 | 2 | 3 | 4;
  /** Whether this cell falls inside the requested window (vs. leading padding). */
  readonly inRange: boolean;
}

export interface CalendarModel {
  readonly cells: CalendarCell[];
  readonly weeks: number;
  readonly monthLabels: { weekIndex: number; label: string }[];
  readonly maxSessionCount: number;
  readonly totalSessions: number;
  readonly totalDays: number; // days with >=1 session
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

function levelFor(count: number, max: number): 0 | 1 | 2 | 3 | 4 {
  if (count <= 0) return 0;
  if (max <= 0) return 1;
  const ratio = count / max;
  if (ratio > 0.66) return 4;
  if (ratio > 0.33) return 3;
  if (ratio > 0.1) return 2;
  return 1;
}

export interface CalendarOptions {
  /** Last day shown (inclusive). Default: latest session start, else today. */
  end?: Date;
  /** Number of week columns. Default 26 (~6 months). */
  weeks?: number;
}

/**
 * Build a weekday-row × week-column grid ending at `end`. The grid always
 * starts on a Sunday so rows align to weekdays. Cells before the first session
 * day (or before the window) are marked `inRange:false` / level 0.
 */
export function buildCalendar(
  sessions: readonly StorySession[],
  opts: CalendarOptions = {},
): CalendarModel {
  const buckets = bucketByDay(sessions);
  const weeks = opts.weeks ?? 26;

  // Resolve the window end.
  let end = opts.end ? startOfDay(opts.end) : null;
  if (!end) {
    let maxT = -Infinity;
    for (const s of sessions) {
      const t = s.start_time ? Date.parse(s.start_time) : NaN;
      if (Number.isFinite(t) && t > maxT) maxT = t;
    }
    end = Number.isFinite(maxT) ? startOfDay(new Date(maxT)) : startOfDay(new Date());
  }

  // Align the last column's Saturday, then walk back (weeks*7 - 1) days to the
  // grid's first Sunday.
  const endSaturday = new Date(end);
  endSaturday.setDate(endSaturday.getDate() + (6 - endSaturday.getDay()));
  const gridStart = new Date(endSaturday);
  gridStart.setDate(gridStart.getDate() - (weeks * 7 - 1));

  const maxSessionCount = Math.max(0, ...[...buckets.values()].map((b) => b.sessionCount));

  const cells: CalendarCell[] = [];
  const monthLabels: { weekIndex: number; label: string }[] = [];
  let lastMonth = -1;
  let totalSessions = 0;
  let totalDays = 0;

  for (let w = 0; w < weeks; w++) {
    for (let dow = 0; dow < 7; dow++) {
      const d = new Date(gridStart);
      d.setDate(d.getDate() + w * 7 + dow);
      const key = dayKey(d);
      const bucket = buckets.get(key);
      const inRange = d <= end && d >= gridStart;
      const sessionCount = bucket?.sessionCount ?? 0;
      if (sessionCount > 0) {
        totalSessions += sessionCount;
        totalDays += 1;
      }
      // Month label: first week whose Sunday introduces a new month.
      if (dow === 0 && d.getMonth() !== lastMonth && d <= end) {
        lastMonth = d.getMonth();
        monthLabels.push({ weekIndex: w, label: MONTHS[d.getMonth()] ?? "" });
      }
      cells.push({
        date: key,
        weekIndex: w,
        dow,
        sessionCount,
        eventCount: bucket?.eventCount ?? 0,
        tokens: bucket?.tokens ?? 0,
        level: inRange ? levelFor(sessionCount, maxSessionCount) : 0,
        inRange,
      });
    }
  }

  return { cells, weeks, monthLabels, maxSessionCount, totalSessions, totalDays };
}

// ── facets ────────────────────────────────────────────────────────────────

export interface FacetValue {
  readonly key: string;
  readonly count: number;
}

export interface Facets {
  readonly projects: FacetValue[];
  readonly hosts: FacetValue[];
  readonly users: FacetValue[];
  readonly branches: FacetValue[];
  readonly statuses: FacetValue[];
  readonly agents: FacetValue[];
}

function tally(sessions: readonly StorySession[], pick: (s: StorySession) => string | null | undefined): FacetValue[] {
  const counts = new Map<string, number>();
  for (const s of sessions) {
    const v = pick(s);
    if (!v) continue;
    counts.set(v, (counts.get(v) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([key, count]) => ({ key, count }))
    .sort((a, b) => b.count - a.count || a.key.localeCompare(b.key));
}

export function computeFacets(sessions: readonly StorySession[]): Facets {
  return {
    projects: tally(sessions, projectKey),
    hosts: tally(sessions, (s) => s.host),
    users: tally(sessions, (s) => s.user),
    branches: tally(sessions, (s) => s.branch),
    statuses: tally(sessions, (s) => s.status),
    agents: tally(sessions, (s) => s.origin_agent),
  };
}

// ── filtering ─────────────────────────────────────────────────────────────

export interface OverviewFilters {
  project?: string;
  host?: string;
  user?: string;
  branch?: string;
  status?: string;
  agent?: string;
  /** YYYY-MM-DD — restrict to sessions started on this day. */
  day?: string;
  /** Free-text; matches label / branch / project / session id (case-insensitive). */
  search?: string;
}

export function applyFilters(sessions: readonly StorySession[], f: OverviewFilters): StorySession[] {
  const q = f.search?.trim().toLowerCase();
  return sessions.filter((s) => {
    if (f.project && projectKey(s) !== f.project) return false;
    if (f.host && s.host !== f.host) return false;
    if (f.user && s.user !== f.user) return false;
    if (f.branch && s.branch !== f.branch) return false;
    if (f.status && s.status !== f.status) return false;
    if (f.agent && s.origin_agent !== f.agent) return false;
    if (f.day && sessionDayKey(s) !== f.day) return false;
    if (q) {
      const hay = `${s.label ?? ""} ${s.branch ?? ""} ${projectKey(s)} ${s.session_id}`.toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });
}

export function hasActiveFilters(f: OverviewFilters): boolean {
  return Boolean(f.project || f.host || f.user || f.branch || f.status || f.agent || f.day || f.search?.trim());
}

// ── sorting ───────────────────────────────────────────────────────────────

export type SortKey = "recent" | "events" | "tokens" | "duration";

export const SORT_LABELS: Record<SortKey, string> = {
  recent: "Most recent",
  events: "Most events",
  tokens: "Most tokens",
  duration: "Longest",
};

export function sortSessions(sessions: readonly StorySession[], key: SortKey): StorySession[] {
  const arr = [...sessions];
  switch (key) {
    case "events":
      return arr.sort((a, b) => (b.event_count ?? 0) - (a.event_count ?? 0));
    case "tokens":
      return arr.sort((a, b) => sessionTokens(b) - sessionTokens(a));
    case "duration":
      return arr.sort((a, b) => sessionDurationMs(b) - sessionDurationMs(a));
    case "recent":
    default:
      return arr.sort((a, b) => {
        const ta = a.last_event ? Date.parse(a.last_event) : 0;
        const tb = b.last_event ? Date.parse(b.last_event) : 0;
        return tb - ta;
      });
  }
}

// ── top-line stats ────────────────────────────────────────────────────────

export interface OverviewStats {
  readonly sessionCount: number;
  readonly eventCount: number;
  readonly tokens: number;
  readonly busiest: StorySession | null; // most events
}

/** Map frecency-ranked ids to their sessions, preserving order, capped.
 *  Ids with no matching session (e.g. deleted/agent) are dropped. */
export function pickRecentSessions(
  sessions: readonly StorySession[],
  recentIds: readonly string[],
  limit = 5,
): StorySession[] {
  const byId = new Map(sessions.map((s) => [s.session_id, s]));
  const out: StorySession[] = [];
  for (const id of recentIds) {
    const s = byId.get(id);
    if (s) out.push(s);
    if (out.length >= limit) break;
  }
  return out;
}

export function computeStats(sessions: readonly StorySession[]): OverviewStats {
  let eventCount = 0;
  let tokens = 0;
  let busiest: StorySession | null = null;
  for (const s of sessions) {
    eventCount += s.event_count ?? 0;
    tokens += sessionTokens(s);
    if (!busiest || (s.event_count ?? 0) > (busiest.event_count ?? 0)) busiest = s;
  }
  return { sessionCount: sessions.length, eventCount, tokens, busiest };
}
