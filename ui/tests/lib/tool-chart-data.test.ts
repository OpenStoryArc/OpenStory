import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { prepareToolChartData, type ToolChartPoint } from "@/lib/tool-chart-data";

// ── Boundary table for prepareToolChartData ──────────────────────
// Covers: empty, single tool, within limit, overflow with "Other", tie-breaking

const BOUNDARY_TABLE: [
  string,                        // description
  Record<string, number>,        // breakdown
  number,                        // maxBars
  ToolChartPoint[],              // expected
][] = [
  // Empty breakdown
  ["empty breakdown", {}, 8, []],

  // Single tool
  ["single tool", { Bash: 42 }, 8, [{ name: "Bash", count: 42 }]],

  // Exactly at limit — no "Other"
  [
    "at limit",
    { Bash: 10, Read: 8, Edit: 5 },
    3,
    [
      { name: "Bash", count: 10 },
      { name: "Read", count: 8 },
      { name: "Edit", count: 5 },
    ],
  ],

  // Over limit → top N-1 + "Other"
  [
    "over limit with Other bucket",
    { Bash: 50, Read: 30, Edit: 20, Write: 10, Glob: 5 },
    3,
    [
      { name: "Bash", count: 50 },
      { name: "Read", count: 30 },
      { name: "Other", count: 35 },
    ],
  ],

  // 12 tools into 8 bars → top 7 + Other
  [
    "12 tools into 8 bars",
    {
      Bash: 100, Read: 80, Edit: 60, Write: 50,
      Glob: 40, Grep: 30, Agent: 20, WebSearch: 15,
      WebFetch: 10, NotebookEdit: 5, TodoWrite: 3, TaskCreate: 2,
    },
    8,
    [
      { name: "Bash", count: 100 },
      { name: "Read", count: 80 },
      { name: "Edit", count: 60 },
      { name: "Write", count: 50 },
      { name: "Glob", count: 40 },
      { name: "Grep", count: 30 },
      { name: "Agent", count: 20 },
      { name: "Other", count: 35 },
    ],
  ],

  // Tie-breaking: same counts, alphabetical order
  [
    "tie-breaking by name",
    { Zebra: 10, Alpha: 10, Middle: 10 },
    2,
    [
      { name: "Alpha", count: 10 },
      { name: "Other", count: 20 },
    ],
  ],
];

describe("prepareToolChartData — boundary table", () => {
  it.each(BOUNDARY_TABLE)(
    "%s",
    (_desc, breakdown, maxBars, expected) => {
      scenario(
        () => ({ breakdown, maxBars }),
        ({ breakdown, maxBars }) => prepareToolChartData(breakdown, maxBars),
        (result) => expect(result).toEqual(expected),
      );
    },
  );
});
