import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  formatSynopsisDuration,
  sortFileImpact,
  fileBasename,
  truncateError,
  deriveSynopsisMetrics,
  type SessionSynopsis,
  type FileImpact,
} from "@/lib/session-detail";

// ── formatSynopsisDuration — boundary table ──────────────────────
// Covers: null, zero, seconds, minutes, minutes+seconds, hours, hours+minutes

const DURATION_TABLE: [string, number | null, string][] = [
  ["null duration", null, "—"],
  ["zero seconds", 0, "—"],
  ["negative", -5, "—"],
  ["seconds only", 45, "45s"],
  ["exactly 1 minute", 60, "1m"],
  ["minutes + seconds", 125, "2m 5s"],
  ["exactly 1 hour", 3600, "1h"],
  ["hours + minutes", 3725, "1h 2m"],
  ["large hours", 7200, "2h"],
  ["hours + minutes no seconds", 5400, "1h 30m"],
];

describe("formatSynopsisDuration — boundary table", () => {
  it.each(DURATION_TABLE)(
    "%s",
    (_desc, input, expected) => {
      scenario(
        () => input,
        (secs) => formatSynopsisDuration(secs),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── sortFileImpact — boundary table ──────────────────────
// Covers: empty, single, multiple sorted by total ops, tie-breaking by name

const SORT_TABLE: [string, FileImpact[], FileImpact[]][] = [
  ["empty array", [], []],
  [
    "single file",
    [{ file: "a.ts", reads: 3, writes: 1 }],
    [{ file: "a.ts", reads: 3, writes: 1 }],
  ],
  [
    "sorted by total desc",
    [
      { file: "low.ts", reads: 1, writes: 0 },
      { file: "high.ts", reads: 5, writes: 3 },
      { file: "mid.ts", reads: 2, writes: 1 },
    ],
    [
      { file: "high.ts", reads: 5, writes: 3 },
      { file: "mid.ts", reads: 2, writes: 1 },
      { file: "low.ts", reads: 1, writes: 0 },
    ],
  ],
  [
    "tie-breaking by name alphabetically",
    [
      { file: "zebra.ts", reads: 3, writes: 0 },
      { file: "alpha.ts", reads: 2, writes: 1 },
    ],
    [
      { file: "alpha.ts", reads: 2, writes: 1 },
      { file: "zebra.ts", reads: 3, writes: 0 },
    ],
  ],
  [
    "all zeros",
    [
      { file: "b.ts", reads: 0, writes: 0 },
      { file: "a.ts", reads: 0, writes: 0 },
    ],
    [
      { file: "a.ts", reads: 0, writes: 0 },
      { file: "b.ts", reads: 0, writes: 0 },
    ],
  ],
];

describe("sortFileImpact — boundary table", () => {
  it.each(SORT_TABLE)(
    "%s",
    (_desc, input, expected) => {
      scenario(
        () => input,
        (files) => sortFileImpact(files),
        (result) => expect(result).toEqual(expected),
      );
    },
  );
});

// ── fileBasename — boundary table ──────────────────────

const BASENAME_TABLE: [string, string, string][] = [
  ["simple filename", "foo.ts", "foo.ts"],
  ["unix path", "src/lib/foo.ts", "foo.ts"],
  ["windows path", "src\\lib\\foo.ts", "foo.ts"],
  ["deep path", "a/b/c/d/e.rs", "e.rs"],
  ["trailing slash", "src/lib/", ""],
];

describe("fileBasename — boundary table", () => {
  it.each(BASENAME_TABLE)(
    "%s",
    (_desc, input, expected) => {
      scenario(
        () => input,
        (path) => fileBasename(path),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── truncateError — boundary table ──────────────────────

const TRUNCATE_TABLE: [string, string, number, string][] = [
  ["short message", "Error!", 200, "Error!"],
  ["exactly at limit", "x".repeat(10), 10, "x".repeat(10)],
  ["over limit", "x".repeat(15), 10, "x".repeat(10) + "…"],
  ["custom limit", "abcdefghij", 5, "abcde…"],
];

describe("truncateError — boundary table", () => {
  it.each(TRUNCATE_TABLE)(
    "%s",
    (_desc, msg, max, expected) => {
      scenario(
        () => ({ msg, max }),
        ({ msg, max }) => truncateError(msg, max),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── deriveSynopsisMetrics ──────────────────────

describe("deriveSynopsisMetrics", () => {
  it("derives all fields from a synopsis", () => {
    const synopsis: SessionSynopsis = {
      session_id: "abc",
      label: "test",
      project_id: "proj",
      project_name: "My Project",
      event_count: 42,
      tool_count: 15,
      error_count: 3,
      first_event: "2026-01-01T00:00:00Z",
      last_event: "2026-01-01T01:00:00Z",
      duration_secs: 3600,
      top_tools: [{ tool: "Read", count: 10 }],
    };

    scenario(
      () => synopsis,
      (s) => deriveSynopsisMetrics(s),
      (m) => {
        expect(m.events).toBe(42);
        expect(m.tools).toBe(15);
        expect(m.errors).toBe(3);
        expect(m.duration).toBe("1h");
      },
    );
  });

  it("handles zero duration synopsis", () => {
    const synopsis: SessionSynopsis = {
      session_id: "abc",
      label: null,
      project_id: null,
      project_name: null,
      event_count: 0,
      tool_count: 0,
      error_count: 0,
      first_event: null,
      last_event: null,
      duration_secs: null,
      top_tools: [],
    };

    scenario(
      () => synopsis,
      (s) => deriveSynopsisMetrics(s),
      (m) => {
        expect(m.events).toBe(0);
        expect(m.duration).toBe("—");
      },
    );
  });
});
