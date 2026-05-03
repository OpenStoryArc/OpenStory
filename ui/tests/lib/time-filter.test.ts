/**
 * Spec: time-filter.ts — pure helpers for the Live tab time window.
 *
 * No DOM, no React. Anchors against an explicit `now` so the
 * assertions don't depend on the wall clock.
 */

import { describe, it, expect } from "vitest";
import {
  TIME_FILTER_LABELS,
  TIME_FILTER_ORDER,
  timeFilterLowerBound,
  timeFilterMatches,
  type TimeFilterKey,
} from "@/lib/time-filter";

// A fixed reference point: Wed 2026-05-06 14:30:00 local.
function refNow(): number {
  // Use a Date built from local-time fields so the calendar boundaries
  // ("today" = midnight local, "week" = local Sunday) work the same on
  // any CI machine.
  const d = new Date();
  d.setFullYear(2026, 4, 6); // May (month is 0-indexed)
  d.setHours(14, 30, 0, 0);
  return d.getTime();
}

describe("TIME_FILTER_ORDER + TIME_FILTER_LABELS", () => {
  it("renders pills in widening-window order", () => {
    expect(TIME_FILTER_ORDER).toEqual(["1h", "today", "week", "all"]);
  });

  it("has a label for every key", () => {
    for (const key of TIME_FILTER_ORDER) {
      expect(TIME_FILTER_LABELS[key]).toBeTruthy();
    }
  });
});

describe("timeFilterLowerBound", () => {
  it("returns 0 for 'all' (no lower bound)", () => {
    expect(timeFilterLowerBound("all", refNow())).toBe(0);
  });

  it("returns now − 1h for '1h'", () => {
    const now = refNow();
    expect(timeFilterLowerBound("1h", now)).toBe(now - 60 * 60 * 1000);
  });

  it("returns midnight local for 'today'", () => {
    const lb = timeFilterLowerBound("today", refNow());
    const d = new Date(lb);
    expect(d.getHours()).toBe(0);
    expect(d.getMinutes()).toBe(0);
    expect(d.getSeconds()).toBe(0);
    expect(d.getDate()).toBe(6); // same calendar day as refNow
  });

  it("returns the most recent Sunday at midnight local for 'week'", () => {
    // 2026-05-06 is a Wednesday → previous Sunday is 2026-05-03.
    const lb = timeFilterLowerBound("week", refNow());
    const d = new Date(lb);
    expect(d.getDay()).toBe(0); // Sunday
    expect(d.getHours()).toBe(0);
  });
});

describe("timeFilterMatches", () => {
  const now = refNow();

  it("matches everything when filter is 'all'", () => {
    expect(timeFilterMatches("2020-01-01T00:00:00Z", "all", now)).toBe(true);
    expect(timeFilterMatches("", "all", now)).toBe(true);
  });

  it("matches a timestamp 30 min ago for '1h'", () => {
    const ts = new Date(now - 30 * 60 * 1000).toISOString();
    expect(timeFilterMatches(ts, "1h", now)).toBe(true);
  });

  it("rejects a timestamp 2h ago for '1h'", () => {
    const ts = new Date(now - 2 * 60 * 60 * 1000).toISOString();
    expect(timeFilterMatches(ts, "1h", now)).toBe(false);
  });

  it("rejects yesterday for 'today' but accepts 6 hours ago", () => {
    const yesterday = new Date(now - 25 * 60 * 60 * 1000).toISOString();
    const sixHoursAgo = new Date(now - 6 * 60 * 60 * 1000).toISOString();
    expect(timeFilterMatches(yesterday, "today", now)).toBe(false);
    expect(timeFilterMatches(sixHoursAgo, "today", now)).toBe(true);
  });

  it("accepts 2 days ago for 'week' but rejects last Saturday", () => {
    // refNow is Wed May 6 14:30 → last Sunday boundary is May 3 00:00.
    // 2 days ago = Mon May 4 14:30 (in window).
    // Sat May 2 14:30 (which is now − 4 days) is *before* the Sunday
    // boundary — out of window.
    const twoDays = new Date(now - 2 * 24 * 60 * 60 * 1000).toISOString();
    expect(timeFilterMatches(twoDays, "week", now)).toBe(true);

    const lastSat = new Date(now - 4 * 24 * 60 * 60 * 1000).toISOString();
    expect(timeFilterMatches(lastSat, "week", now)).toBe(false);
  });

  it("never matches an empty timestamp on a non-'all' filter", () => {
    const filters: TimeFilterKey[] = ["1h", "today", "week"];
    for (const f of filters) {
      expect(timeFilterMatches("", f, now)).toBe(false);
    }
  });

  it("never matches an unparseable timestamp on a non-'all' filter", () => {
    expect(timeFilterMatches("not-a-date", "1h", now)).toBe(false);
  });
});
