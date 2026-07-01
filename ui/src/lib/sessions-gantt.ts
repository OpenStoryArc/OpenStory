/** Pure model for the Fleet Gantt — concurrent sessions packed into lanes on a
 *  time axis. Greedy interval partitioning (sessions that overlap in wall-clock
 *  get separate lanes); sessions band by a group dimension. Deterministic
 *  (sort by start, tie by id) so it's unit-testable; `nowMs` is passed in so
 *  ongoing sessions get a defined end without touching the clock in the model. */

import type { StorySession } from "@/lib/story-api";
import { projectKey } from "@/lib/sessions-overview";
import type { GroupDim } from "@/lib/sessions-canvas";

export interface GanttBar {
  readonly id: string;
  readonly label: string;
  readonly startMs: number;
  readonly endMs: number;
  readonly ongoing: boolean;
  readonly agent: string;
  readonly band: string;
  /** absolute lane row across all bands. */
  readonly lane: number;
}

export interface GanttBand {
  readonly name: string;
  readonly laneStart: number;
  readonly laneCount: number;
}

export interface GanttModel {
  readonly bars: GanttBar[];
  readonly bands: GanttBand[];
  readonly laneCount: number;
  readonly domain: [number, number];
}

function bandValue(s: StorySession, dim: GroupDim): string {
  switch (dim) {
    case "user": return s.user || "unknown";
    case "agent": return s.origin_agent || "unknown";
    case "status": return s.status || "unknown";
    case "host": return s.host || "unknown";
    case "day": return (s.start_time ?? "").slice(0, 10) || "undated";
    case "project": return projectKey(s);
  }
}

export function buildGantt(sessions: readonly StorySession[], groupBy: GroupDim, nowMs: number): GanttModel {
  interface Row { s: StorySession; start: number; end: number; ongoing: boolean; band: string }
  const rows: Row[] = [];
  for (const s of sessions) {
    const start = s.start_time ? Date.parse(s.start_time) : NaN;
    if (!Number.isFinite(start)) continue;
    const ongoing = s.status === "ongoing" || !s.last_event;
    const rawEnd = s.last_event ? Date.parse(s.last_event) : NaN;
    const end = ongoing ? nowMs : Number.isFinite(rawEnd) ? Math.max(rawEnd, start) : start;
    rows.push({ s, start, end, ongoing, band: bandValue(s, groupBy) });
  }

  // bands ordered by size desc, then name
  const byBand = new Map<string, Row[]>();
  for (const r of rows) (byBand.get(r.band) ?? byBand.set(r.band, []).get(r.band)!).push(r);
  const bandEntries = [...byBand.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]));

  const bars: GanttBar[] = [];
  const bands: GanttBand[] = [];
  let laneCursor = 0;

  for (const [band, list] of bandEntries) {
    // greedy lane packing within the band
    list.sort((a, b) => a.start - b.start || a.s.session_id.localeCompare(b.s.session_id));
    const laneEnds: number[] = []; // lastEnd per local lane
    for (const r of list) {
      let lane = laneEnds.findIndex((e) => e <= r.start);
      if (lane === -1) { lane = laneEnds.length; laneEnds.push(r.end); }
      else laneEnds[lane] = r.end;
      bars.push({
        id: r.s.session_id,
        label: r.s.label || r.s.session_id.slice(0, 8),
        startMs: r.start, endMs: r.end, ongoing: r.ongoing,
        agent: r.s.origin_agent || "unknown", band,
        lane: laneCursor + lane,
      });
    }
    const laneCount = Math.max(laneEnds.length, 1);
    bands.push({ name: band, laneStart: laneCursor, laneCount });
    laneCursor += laneCount;
  }

  const starts = bars.map((b) => b.startMs);
  const ends = bars.map((b) => b.endMs);
  return {
    bars,
    bands,
    laneCount: laneCursor,
    domain: [starts.length ? Math.min(...starts) : 0, ends.length ? Math.max(...ends) : 0],
  };
}
