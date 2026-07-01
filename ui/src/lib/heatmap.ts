/** Pure model for the contribution heatmap (2D grid + 3D stacks).
 *
 *  Turns the flat session list into a GitHub-style calendar: `weeks` columns
 *  (oldest → newest, left → right) × 7 day-rows (Sun → Sat). Each day-cell holds
 *  that day's sessions sorted LARGEST → SMALLEST (so the 3D stack puts the
 *  biggest session at the base). Side-effect-free (nowMs is injected) so the
 *  calendar/window math is exhaustively unit-tested. */

import type { StorySession } from "@/lib/story-api";
import { sessionDayKey, sessionTokens, dayKey } from "@/lib/sessions-overview";
import { sessionTitle } from "@/lib/session-title";

export interface HeatSession {
  readonly id: string;
  readonly title: string;
  readonly events: number;
  readonly tokens: number;
  readonly agent: string;
  readonly project: string;
}

export interface HeatCell {
  readonly date: string; // YYYY-MM-DD (local)
  readonly week: number; // column, 0 = oldest … weeks-1 = newest
  readonly day: number; // row, 0 = Sun … 6 = Sat
  readonly sessions: HeatSession[]; // largest → smallest (stack base → top)
  readonly count: number;
  readonly events: number;
  readonly tokens: number;
  /** true when the cell's date is in the past/today (not a future padding cell). */
  readonly present: boolean;
}

export interface HeatGrid {
  readonly cells: HeatCell[];
  readonly weeks: number;
  readonly maxCount: number; // busiest day's session count (height/color scale)
  readonly totalSessions: number;
  readonly startDate: string;
  readonly endDate: string;
}

export type HeatMetric = "sessions" | "events" | "tokens";

function startOfWeek(ms: number): Date {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - d.getDay()); // back to Sunday
  return d;
}

export function buildHeatmap(
  sessions: readonly StorySession[],
  { nowMs, weeks = 26, metric = "sessions" }: { nowMs: number; weeks?: number; metric?: HeatMetric },
): HeatGrid {
  const w = Math.max(1, weeks);

  // bucket sessions by local day
  const byDay = new Map<string, HeatSession[]>();
  for (const s of sessions) {
    const key = sessionDayKey(s);
    if (!key) continue;
    (byDay.get(key) ?? byDay.set(key, []).get(key)!).push({
      id: s.session_id,
      title: sessionTitle(s),
      events: s.event_count ?? 0,
      tokens: sessionTokens(s),
      agent: s.origin_agent || "unknown",
      project: s.project_name || s.project_id || "unknown",
    });
  }

  // grid window: w weeks ending at the week containing nowMs (Sun-aligned)
  const gridStart = startOfWeek(nowMs);
  gridStart.setDate(gridStart.getDate() - (w - 1) * 7);
  const todayKey = dayKey(new Date(nowMs));

  const cells: HeatCell[] = [];
  let maxCount = 0;
  let total = 0;
  for (let col = 0; col < w; col++) {
    for (let row = 0; row < 7; row++) {
      const d = new Date(gridStart);
      d.setDate(gridStart.getDate() + col * 7 + row);
      const key = dayKey(d);
      const list = (byDay.get(key) ?? []).slice().sort((a, b) => {
        const av = metric === "tokens" ? a.tokens : a.events;
        const bv = metric === "tokens" ? b.tokens : b.events;
        return bv - av || (a.id < b.id ? -1 : 1);
      });
      const events = list.reduce((n, x) => n + x.events, 0);
      const tokens = list.reduce((n, x) => n + x.tokens, 0);
      maxCount = Math.max(maxCount, list.length);
      total += list.length;
      cells.push({
        date: key, week: col, day: row, sessions: list,
        count: list.length, events, tokens,
        present: key <= todayKey,
      });
    }
  }

  const startDate = cells[0]?.date ?? todayKey;
  const endDate = cells[cells.length - 1]?.date ?? todayKey;
  return { cells, weeks: w, maxCount, totalSessions: total, startDate, endDate };
}

/** 0..4 quantile-ish intensity level for the 2D grid color (GitHub-style). */
export function heatLevel(count: number, maxCount: number): 0 | 1 | 2 | 3 | 4 {
  if (count <= 0) return 0;
  if (maxCount <= 1) return 4;
  const r = count / maxCount;
  if (r <= 0.25) return 1;
  if (r <= 0.5) return 2;
  if (r <= 0.75) return 3;
  return 4;
}
