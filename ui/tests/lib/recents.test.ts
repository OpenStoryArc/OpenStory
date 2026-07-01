import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { recordVisit, rankRecents, EMPTY_RECENTS, type RecentsState } from "@/lib/recents";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

describe("recordVisit", () => {
  it("adds a new entry with count 1", () => {
    scenario(
      () => recordVisit(EMPTY_RECENTS, "s1", 1000),
      (state) => state.entries,
      (entries) => {
        expect(entries).toHaveLength(1);
        expect(entries[0]).toEqual({ id: "s1", count: 1, lastVisit: 1000 });
      },
    );
  });

  it("bumps count and lastVisit on a repeat visit", () => {
    scenario(
      () => recordVisit(recordVisit(EMPTY_RECENTS, "s1", 1000), "s1", 5000),
      (state) => state.entries.find((e) => e.id === "s1"),
      (entry) => {
        expect(entry?.count).toBe(2);
        expect(entry?.lastVisit).toBe(5000);
      },
    );
  });

  it("caps the stored history, dropping the lowest-frecency entry", () => {
    scenario(
      () => {
        let state: RecentsState = EMPTY_RECENTS;
        // Fill beyond the cap; each visited once, increasing timestamps.
        for (let i = 0; i < 60; i++) state = recordVisit(state, `s${i}`, i * 1000);
        return state;
      },
      (state) => state,
      (state) => {
        expect(state.entries.length).toBeLessThanOrEqual(50);
        // the oldest (s0) should have been evicted
        expect(state.entries.find((e) => e.id === "s0")).toBeUndefined();
        // the newest survives
        expect(state.entries.find((e) => e.id === "s59")).toBeDefined();
      },
    );
  });
});

describe("rankRecents", () => {
  it("orders the most recently visited first across recency buckets", () => {
    scenario(
      () => {
        const now = 100 * DAY;
        let state = recordVisit(EMPTY_RECENTS, "old", now - 10 * DAY);
        state = recordVisit(state, "recent", now - 1000);
        state = recordVisit(state, "yesterday", now - 1 * DAY - 1000);
        return { state, now };
      },
      ({ state, now }) => rankRecents(state, now),
      (ranked) => expect(ranked).toEqual(["recent", "yesterday", "old"]),
    );
  });

  it("breaks ties within the same recency bucket by visit frequency", () => {
    scenario(
      () => {
        const now = 100 * DAY;
        // both visited within the last hour, but 'frequent' was visited 3×
        let state = recordVisit(EMPTY_RECENTS, "rare", now - 100);
        state = recordVisit(state, "frequent", now - 300);
        state = recordVisit(state, "frequent", now - 200);
        state = recordVisit(state, "frequent", now - 150);
        return { state, now };
      },
      ({ state, now }) => rankRecents(state, now),
      (ranked) => expect(ranked[0]).toBe("frequent"),
    );
  });

  it("returns an empty list for no history", () => {
    expect(rankRecents(EMPTY_RECENTS, 1000)).toEqual([]);
  });
});
