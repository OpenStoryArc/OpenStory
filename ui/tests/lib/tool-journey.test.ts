import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  groupConsecutiveTools,
  buildJourneySequence,
  formatGapDuration,
  truncateFilePath,
  type ToolStep,
  type ToolGroup,
} from "@/lib/tool-journey";

// ── groupConsecutiveTools — boundary table ──────────────────────
// Covers: empty, single step, consecutive same-tool, alternating, dedup files

describe("groupConsecutiveTools", () => {
  it("returns empty for no steps", () => {
    scenario(
      () => [] as ToolStep[],
      (steps) => groupConsecutiveTools(steps),
      (groups) => expect(groups).toEqual([]),
    );
  });

  it("single step becomes single group", () => {
    scenario(
      () => [{ tool: "Read", file: "a.ts", timestamp: "2026-01-01T00:00:00Z" }] as ToolStep[],
      (steps) => groupConsecutiveTools(steps),
      (groups) => {
        expect(groups).toHaveLength(1);
        expect(groups[0]!.tool).toBe("Read");
        expect(groups[0]!.count).toBe(1);
        expect(groups[0]!.files).toEqual(["a.ts"]);
      },
    );
  });

  it("groups consecutive same-tool steps", () => {
    const steps: ToolStep[] = [
      { tool: "Read", file: "a.ts", timestamp: "2026-01-01T00:00:00Z" },
      { tool: "Read", file: "b.ts", timestamp: "2026-01-01T00:00:01Z" },
      { tool: "Read", file: "a.ts", timestamp: "2026-01-01T00:00:02Z" },
      { tool: "Edit", file: "a.ts", timestamp: "2026-01-01T00:00:03Z" },
    ];

    scenario(
      () => steps,
      (s) => groupConsecutiveTools(s),
      (groups) => {
        expect(groups).toHaveLength(2);
        expect(groups[0]!.tool).toBe("Read");
        expect(groups[0]!.count).toBe(3);
        expect(groups[0]!.files).toEqual(["a.ts", "b.ts"]); // deduped
        expect(groups[1]!.tool).toBe("Edit");
        expect(groups[1]!.count).toBe(1);
      },
    );
  });

  it("alternating tools produce separate groups", () => {
    const steps: ToolStep[] = [
      { tool: "Read", file: "a.ts", timestamp: "2026-01-01T00:00:00Z" },
      { tool: "Edit", file: "a.ts", timestamp: "2026-01-01T00:00:01Z" },
      { tool: "Read", file: "b.ts", timestamp: "2026-01-01T00:00:02Z" },
    ];

    scenario(
      () => steps,
      (s) => groupConsecutiveTools(s),
      (groups) => {
        expect(groups).toHaveLength(3);
        expect(groups.map((g) => g.tool)).toEqual(["Read", "Edit", "Read"]);
      },
    );
  });

  it("handles null files", () => {
    const steps: ToolStep[] = [
      { tool: "Bash", file: null, timestamp: "2026-01-01T00:00:00Z" },
      { tool: "Bash", file: null, timestamp: "2026-01-01T00:00:01Z" },
    ];

    scenario(
      () => steps,
      (s) => groupConsecutiveTools(s),
      (groups) => {
        expect(groups).toHaveLength(1);
        expect(groups[0]!.files).toEqual([]);
      },
    );
  });
});

// ── buildJourneySequence — boundary table ──────────────────────

describe("buildJourneySequence", () => {
  it("returns empty for no groups", () => {
    scenario(
      () => [] as ToolGroup[],
      (groups) => buildJourneySequence(groups),
      (elements) => expect(elements).toEqual([]),
    );
  });

  it("single group, no gaps", () => {
    const groups: ToolGroup[] = [
      { tool: "Read", count: 3, files: ["a.ts"], startTime: "2026-01-01T00:00:00Z", endTime: "2026-01-01T00:00:02Z" },
    ];

    scenario(
      () => groups,
      (g) => buildJourneySequence(g),
      (elements) => {
        expect(elements).toHaveLength(1);
        expect(elements[0]!.kind).toBe("group");
      },
    );
  });

  it("inserts gap when threshold exceeded", () => {
    const groups: ToolGroup[] = [
      { tool: "Read", count: 1, files: [], startTime: "2026-01-01T00:00:00Z", endTime: "2026-01-01T00:00:05Z" },
      { tool: "Edit", count: 1, files: [], startTime: "2026-01-01T00:01:00Z", endTime: "2026-01-01T00:01:05Z" },
    ];

    scenario(
      () => groups,
      (g) => buildJourneySequence(g, 30_000), // 30s threshold
      (elements) => {
        expect(elements).toHaveLength(3);
        expect(elements[0]!.kind).toBe("group");
        expect(elements[1]!.kind).toBe("gap");
        expect(elements[2]!.kind).toBe("group");
        if (elements[1]!.kind === "gap") {
          expect(elements[1]!.gap.durationMs).toBe(55_000);
        }
      },
    );
  });

  it("no gap when under threshold", () => {
    const groups: ToolGroup[] = [
      { tool: "Read", count: 1, files: [], startTime: "2026-01-01T00:00:00Z", endTime: "2026-01-01T00:00:05Z" },
      { tool: "Edit", count: 1, files: [], startTime: "2026-01-01T00:00:10Z", endTime: "2026-01-01T00:00:15Z" },
    ];

    scenario(
      () => groups,
      (g) => buildJourneySequence(g, 30_000),
      (elements) => {
        expect(elements).toHaveLength(2);
        expect(elements.every((e) => e.kind === "group")).toBe(true);
      },
    );
  });
});

// ── formatGapDuration — boundary table ──────────────────────

const GAP_TABLE: [string, number, string][] = [
  ["milliseconds", 500, "500ms"],
  ["seconds", 5000, "5s"],
  ["minutes", 120_000, "2m"],
  ["hours", 7_200_000, "2h"],
  ["just under a minute", 59_000, "59s"],
  ["exactly a minute", 60_000, "1m"],
];

describe("formatGapDuration — boundary table", () => {
  it.each(GAP_TABLE)(
    "%s",
    (_desc, input, expected) => {
      scenario(
        () => input,
        (ms) => formatGapDuration(ms),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── truncateFilePath — boundary table ──────────────────────

const TRUNCATE_TABLE: [string, string, number, string][] = [
  ["simple basename", "foo.ts", 30, "foo.ts"],
  ["extracts basename from path", "src/lib/foo.ts", 30, "foo.ts"],
  ["windows path", "src\\lib\\foo.ts", 30, "foo.ts"],
  ["truncates long basename", "very-long-filename-that-exceeds.ts", 15, "very-long-file…"],
  ["short limit", "abcdef.ts", 5, "abcd…"],
];

describe("truncateFilePath — boundary table", () => {
  it.each(TRUNCATE_TABLE)(
    "%s",
    (_desc, path, maxLen, expected) => {
      scenario(
        () => ({ path, maxLen }),
        ({ path, maxLen }) => truncateFilePath(path, maxLen),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});
