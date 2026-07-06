import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import type { StorySession } from "@/lib/story-api";
import {
  dayKey,
  sessionDayKey,
  sessionDurationMs,
  sessionTokens,
  projectKey,
  bucketByDay,
  buildCalendar,
  computeFacets,
  applyFilters,
  hasActiveFilters,
  sortSessions,
  computeStats,
  pickRecentSessions,
  levelThresholds,
  levelForCount,
} from "@/lib/sessions-overview";

function sess(p: Partial<StorySession> & { session_id: string }): StorySession {
  return { status: "completed", ...p };
}

// Fixed local dates (avoid DST edges).
const A = "2026-06-10T09:00:00.000Z";
const A_END = "2026-06-10T09:30:00.000Z";
const B = "2026-06-11T14:00:00.000Z";

describe("time + scalar helpers", () => {
  it("formats a day key and derives it from a session", () => {
    scenario(
      () => sess({ session_id: "s1", start_time: "2026-06-10T09:00:00.000Z" }),
      (s) => ({ key: sessionDayKey(s), fromDate: dayKey(new Date(2026, 5, 10)) }),
      (r) => {
        expect(r.fromDate).toBe("2026-06-10");
        expect(r.key).toBe(dayKey(new Date("2026-06-10T09:00:00.000Z")));
      },
    );
  });

  it("computes duration, tokens, and project key with fallbacks", () => {
    scenario(
      () => sess({ session_id: "s1", start_time: A, last_event: A_END, total_input_tokens: 100, total_output_tokens: 40, project_id: "proj-x" }),
      (s) => ({ dur: sessionDurationMs(s), tok: sessionTokens(s), proj: projectKey(s) }),
      (r) => {
        expect(r.dur).toBe(30 * 60 * 1000);
        expect(r.tok).toBe(140);
        expect(r.proj).toBe("proj-x"); // falls back to id when name missing
      },
    );
  });

  it("returns 0 duration when timestamps are missing", () => {
    scenario(
      () => sess({ session_id: "s1", start_time: A }),
      (s) => sessionDurationMs(s),
      (d) => expect(d).toBe(0),
    );
  });
});

describe("bucketByDay", () => {
  it("aggregates sessions started on the same local day", () => {
    scenario(
      () => [
        sess({ session_id: "s1", start_time: A, event_count: 10, total_output_tokens: 5 }),
        sess({ session_id: "s2", start_time: "2026-06-10T20:00:00.000Z", event_count: 3, total_output_tokens: 2 }),
        sess({ session_id: "s3", start_time: B, event_count: 7 }),
      ],
      (sessions) => bucketByDay(sessions),
      (buckets) => {
        const dayA = dayKey(new Date(A));
        expect(buckets.get(dayA)?.sessionCount).toBe(2);
        expect(buckets.get(dayA)?.eventCount).toBe(13);
        expect(buckets.get(dayA)?.sessionIds).toEqual(["s1", "s2"]);
      },
    );
  });
});

describe("buildCalendar", () => {
  it("produces a weeks×7 grid ending on/after the last session, aligned to weekdays", () => {
    scenario(
      () => buildCalendar(
        [sess({ session_id: "s1", start_time: A }), sess({ session_id: "s2", start_time: B })],
        { end: new Date(2026, 5, 13), weeks: 8 },
      ),
      (model) => model,
      (model) => {
        expect(model.cells).toHaveLength(8 * 7);
        // grid starts on a Sunday (dow 0) and the first row is all dow 0
        expect(model.cells[0]?.dow).toBe(0);
        expect(model.weeks).toBe(8);
        expect(model.totalSessions).toBe(2);
      },
    );
  });

  it("assigns higher intensity levels to busier days", () => {
    scenario(
      () => buildCalendar(
        [
          ...Array.from({ length: 5 }, (_, i) => sess({ session_id: `busy${i}`, start_time: A })),
          sess({ session_id: "quiet", start_time: B }),
        ],
        { end: new Date(2026, 5, 13), weeks: 8 },
      ),
      (model) => {
        const busy = model.cells.find((c) => c.date === dayKey(new Date(A)));
        const quiet = model.cells.find((c) => c.date === dayKey(new Date(B)));
        return { busyLevel: busy?.level ?? -1, quietLevel: quiet?.level ?? -1 };
      },
      (r) => {
        expect(r.busyLevel).toBeGreaterThan(r.quietLevel);
        expect(r.quietLevel).toBeGreaterThanOrEqual(1);
      },
    );
  });
});

describe("quantile intensity leveling (outlier-robust)", () => {
  it("keeps low-but-nonzero days visible (level >= 1) despite a huge outlier day", () => {
    scenario(
      // heavy-tailed like the real data: lots of 1-2 session days + one 100 day
      () => levelThresholds([1, 1, 1, 2, 2, 3, 5, 8, 100]),
      (thresholds) => ({
        thresholds,
        low: levelForCount(1, thresholds),
        mid: levelForCount(5, thresholds),
        huge: levelForCount(100, thresholds),
      }),
      (r) => {
        // a single-session day is still a visible level 1 (old linear scale
        // would crush it toward 0 against a max of 100)
        expect(r.low).toBeGreaterThanOrEqual(1);
        expect(r.huge).toBe(4);
        expect(r.mid).toBeGreaterThan(r.low);
        expect(r.mid).toBeLessThan(r.huge);
      },
    );
  });

  it("returns 0 for empty days and handles a uniform distribution", () => {
    const t = levelThresholds([4, 4, 4]);
    expect(levelForCount(0, t)).toBe(0);
    expect(levelForCount(4, t)).toBe(1);
  });
});

describe("computeFacets", () => {
  it("tallies distinct values sorted by frequency", () => {
    scenario(
      () => [
        sess({ session_id: "s1", host: "a1", user: "max", origin_agent: "claude-code", status: "completed" }),
        sess({ session_id: "s2", host: "a1", user: "max", origin_agent: "claude-code", status: "ongoing" }),
        sess({ session_id: "s3", host: "b2", user: "katie", origin_agent: "codex", status: "completed" }),
      ],
      (sessions) => computeFacets(sessions),
      (facets) => {
        expect(facets.hosts[0]).toEqual({ key: "a1", count: 2 });
        expect(facets.users.map((u) => u.key)).toEqual(["max", "katie"]);
        expect(facets.agents.find((a) => a.key === "codex")?.count).toBe(1);
      },
    );
  });
});

describe("when filtering by a rolling date range", () => {
  // NOW pinned mid-day 2026-06-12; s1's last activity is 2 days back, s2's is 1 day back.
  const NOW = new Date("2026-06-12T12:00:00.000Z").getTime();
  const RANGED = [
    sess({ session_id: "old", start_time: "2026-05-20T09:00:00.000Z", last_event: "2026-05-20T10:00:00.000Z" }), // 23d back: outside 7d, inside 30d
    sess({ session_id: "recent", start_time: A, last_event: B }),
  ];

  it("should keep only sessions whose last activity falls inside the range", () => {
    scenario(
      () => applyFilters(RANGED, { range: "7d" }, NOW),
      (r) => r.map((s) => s.session_id),
      (ids) => expect(ids).toEqual(["recent"]),
    );
  });

  it("should fall back to start_time when a session has no last_event", () => {
    scenario(
      () => applyFilters([sess({ session_id: "s", start_time: B })], { range: "7d" }, NOW),
      (r) => r.map((s) => s.session_id),
      (ids) => expect(ids).toEqual(["s"]),
    );
  });

  it("should compose range with facets (logical AND)", () => {
    scenario(
      () => applyFilters(RANGED, { range: "30d", status: "completed" }, NOW),
      (r) => r.map((s) => s.session_id),
      (ids) => expect(ids).toEqual(["old", "recent"]),
    );
  });

  it("should count a set range as an active filter", () => {
    expect(hasActiveFilters({ range: "7d" })).toBe(true);
    expect(hasActiveFilters({})).toBe(false);
  });
});

describe("applyFilters", () => {
  const SESSIONS = [
    sess({ session_id: "s1", start_time: A, host: "a1", user: "max", branch: "main", status: "completed", label: "fix login bug", project_name: "OpenStory" }),
    sess({ session_id: "s2", start_time: B, host: "b2", user: "katie", branch: "feat/x", status: "ongoing", label: "add calendar", project_name: "OpenStory" }),
  ];

  it("filters by a single facet", () => {
    scenario(
      () => applyFilters(SESSIONS, { host: "a1" }),
      (r) => r.map((s) => s.session_id),
      (ids) => expect(ids).toEqual(["s1"]),
    );
  });

  it("filters by day", () => {
    scenario(
      () => applyFilters(SESSIONS, { day: dayKey(new Date(B)) }),
      (r) => r.map((s) => s.session_id),
      (ids) => expect(ids).toEqual(["s2"]),
    );
  });

  it("matches free-text search across label and branch", () => {
    scenario(
      () => ({ byLabel: applyFilters(SESSIONS, { search: "calendar" }), byBranch: applyFilters(SESSIONS, { search: "feat/x" }) }),
      (r) => r,
      (r) => {
        expect(r.byLabel.map((s) => s.session_id)).toEqual(["s2"]);
        expect(r.byBranch.map((s) => s.session_id)).toEqual(["s2"]);
      },
    );
  });

  it("composes multiple filters with logical AND", () => {
    scenario(
      () => applyFilters(SESSIONS, { host: "a1", status: "ongoing" }),
      (r) => r.length,
      (n) => expect(n).toBe(0),
    );
  });

  it("detects active filters", () => {
    expect(hasActiveFilters({})).toBe(false);
    expect(hasActiveFilters({ search: "  " })).toBe(false);
    expect(hasActiveFilters({ host: "a1" })).toBe(true);
    expect(hasActiveFilters({ day: "2026-06-10" })).toBe(true);
  });
});

describe("pickRecentSessions", () => {
  const SESSIONS = [
    sess({ session_id: "a" }),
    sess({ session_id: "b" }),
    sess({ session_id: "c" }),
  ];

  it("returns sessions in recent-id order, dropping unknown ids", () => {
    scenario(
      () => pickRecentSessions(SESSIONS, ["c", "zzz", "a"]),
      (r) => r.map((s) => s.session_id),
      (ids) => expect(ids).toEqual(["c", "a"]),
    );
  });

  it("caps the result at the limit", () => {
    scenario(
      () => pickRecentSessions(SESSIONS, ["a", "b", "c"], 2),
      (r) => r.map((s) => s.session_id),
      (ids) => expect(ids).toEqual(["a", "b"]),
    );
  });
});

describe("sortSessions + computeStats", () => {
  const SESSIONS = [
    sess({ session_id: "small", start_time: A, last_event: A_END, event_count: 5, total_output_tokens: 10 }),
    sess({ session_id: "big", start_time: A, last_event: "2026-06-10T12:00:00.000Z", event_count: 500, total_output_tokens: 9000 }),
    sess({ session_id: "mid", start_time: B, last_event: "2026-06-11T14:10:00.000Z", event_count: 50, total_output_tokens: 100 }),
  ];

  it("sorts by events, tokens, and duration", () => {
    scenario(
      () => ({
        events: sortSessions(SESSIONS, "events").map((s) => s.session_id),
        tokens: sortSessions(SESSIONS, "tokens").map((s) => s.session_id),
        duration: sortSessions(SESSIONS, "duration").map((s) => s.session_id),
      }),
      (r) => r,
      (r) => {
        expect(r.events[0]).toBe("big");
        expect(r.tokens[0]).toBe("big");
        expect(r.duration[0]).toBe("big"); // 3h span
      },
    );
  });

  it("identifies the busiest session in stats", () => {
    scenario(
      () => computeStats(SESSIONS),
      (stats) => stats,
      (stats) => {
        expect(stats.sessionCount).toBe(3);
        expect(stats.eventCount).toBe(555);
        expect(stats.busiest?.session_id).toBe("big");
      },
    );
  });
});
