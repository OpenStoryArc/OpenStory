import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { fuzzyMatch, rankItems } from "@/lib/command-palette";

describe("fuzzyMatch", () => {
  it("matches a subsequence and reports positions", () => {
    scenario(
      () => fuzzyMatch("ov", "Overview"),
      (r) => r,
      (r) => {
        expect(r).not.toBeNull();
        expect(r!.positions).toEqual([0, 1]);
      },
    );
  });

  it("returns null when the query is not a subsequence", () => {
    expect(fuzzyMatch("zzz", "Overview")).toBeNull();
    expect(fuzzyMatch("ovx", "Overview")).toBeNull();
  });

  it("treats an empty query as a neutral match", () => {
    const r = fuzzyMatch("", "anything");
    expect(r).not.toBeNull();
    expect(r!.score).toBe(0);
  });

  it("scores a prefix/consecutive match higher than a scattered one", () => {
    const prefix = fuzzyMatch("sess", "session calendar")!;
    const scattered = fuzzyMatch("sess", "serialize expression string set")!;
    expect(prefix.score).toBeGreaterThan(scattered.score);
  });

  it("rewards word-boundary matches (acronym-style)", () => {
    const boundary = fuzzyMatch("sc", "Session Calendar")!; // S…C at word starts
    const midword = fuzzyMatch("sc", "miscellaneous")!; // 'sc' mid-word
    expect(boundary.score).toBeGreaterThan(midword.score);
  });

  it("is case-insensitive", () => {
    expect(fuzzyMatch("OVER", "overview")).not.toBeNull();
  });
});

describe("rankItems", () => {
  const ITEMS = ["Overview", "Explore", "Live", "Story", "Session Calendar", "Admin"];

  it("ranks the best fuzzy matches first and drops non-matches", () => {
    scenario(
      () => rankItems("ov", ITEMS, (s) => s),
      (r) => r,
      (r) => {
        expect(r[0]).toBe("Overview");
        expect(r).not.toContain("Admin");
      },
    );
  });

  it("returns items unchanged (capped) for an empty query", () => {
    scenario(
      () => rankItems("", ITEMS, (s) => s, 3),
      (r) => r,
      (r) => expect(r).toEqual(["Overview", "Explore", "Live"]),
    );
  });

  it("respects the result limit", () => {
    expect(rankItems("e", ITEMS, (s) => s, 2).length).toBeLessThanOrEqual(2);
  });

  it("matches against combined searchable text (title + keywords)", () => {
    const items = [
      { title: "Live", keys: "stream realtime" },
      { title: "Overview", keys: "calendar heatmap sessions" },
    ];
    const r = rankItems("heatmap", items, (i) => `${i.title} ${i.keys}`);
    expect(r[0]?.title).toBe("Overview");
  });
});
