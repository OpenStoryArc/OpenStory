/** Pure model for the Events-over-Time Streamgraph (Lab candidate
 *  `events-streamgraph`). Buckets each session's event_count by the day it
 *  started, grouped by agent, into contiguous daily rows ready for d3's
 *  stack() + stackOffsetWiggle. Agents beyond topAgents fold into "other" so
 *  the long tail doesn't shatter the bands. Side-effect-free → unit-tested;
 *  sessions-list only (no records). */

interface StreamSession {
  readonly origin_agent?: string | null;
  readonly event_count?: number | null;
  readonly start_time?: string | null;
  readonly last_event?: string | null;
}

export type StreamRow = Record<string, number> & { day: string };

export interface Streamgraph {
  readonly days: string[];    // contiguous ISO dates (YYYY-MM-DD), ascending
  readonly agents: string[];  // stack keys, by total events desc (+ "other")
  readonly rows: StreamRow[];
  readonly maxTotal: number;  // largest single-day total (for scaling)
  readonly totalEvents: number;
}

const OTHER = "other";

/** Next ISO day after an ISO date, computed in UTC (arg-form Date is allowed). */
function nextDay(iso: string): string {
  const d = new Date(`${iso}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + 1);
  return d.toISOString().slice(0, 10);
}

export function buildStreamgraph(
  sessions: readonly StreamSession[],
  { topAgents = 8 }: { topAgents?: number } = {},
): Streamgraph {
  const dayOf = (s: StreamSession) => (s.start_time || s.last_event || "").slice(0, 10);
  const dated = sessions.filter((s) => /^\d{4}-\d{2}-\d{2}$/.test(dayOf(s)));
  if (dated.length === 0) return { days: [], agents: [], rows: [], maxTotal: 0, totalEvents: 0 };

  // rank agents by total events; fold the tail into "other"
  const agentVol = new Map<string, number>();
  for (const s of dated) agentVol.set(s.origin_agent || "unknown", (agentVol.get(s.origin_agent || "unknown") ?? 0) + (s.event_count ?? 0));
  const ranked = [...agentVol.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map((e) => e[0]);
  const top = ranked.slice(0, topAgents);
  const keep = new Set(top);
  const folded = ranked.length > top.length;
  const agents = folded ? [...top, OTHER] : [...top];
  const colOf = (agent: string) => (keep.has(agent) ? agent : OTHER);

  // aggregate events per (day, column)
  const perDay = new Map<string, Map<string, number>>();
  for (const s of dated) {
    const day = dayOf(s);
    const col = colOf(s.origin_agent || "unknown");
    const row = perDay.get(day) ?? new Map<string, number>();
    row.set(col, (row.get(col) ?? 0) + (s.event_count ?? 0));
    perDay.set(day, row);
  }

  // contiguous day axis from min → max
  const present = [...perDay.keys()].sort();
  const days: string[] = [];
  for (let d = present[0]!; d <= present[present.length - 1]!; d = nextDay(d)) {
    days.push(d);
    if (days.length > 5000) break; // safety bound on span
  }

  let maxTotal = 0, totalEvents = 0;
  const rows: StreamRow[] = days.map((day) => {
    const src = perDay.get(day);
    const row = { day } as StreamRow;
    let total = 0;
    for (const a of agents) { const v = src?.get(a) ?? 0; row[a] = v; total += v; }
    if (total > maxTotal) maxTotal = total;
    totalEvents += total;
    return row;
  });

  return { days, agents, rows, maxTotal, totalEvents };
}
