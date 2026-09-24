/** dora — the four keys as tiles, from `scripts/dora.py --write` (D-06). */

import { describe, expect, it } from "vitest";
import { tilesFor, windowsOf, type DoraJson } from "@/lib/dora";

const json: DoraJson = {
  generated_at: "2026-09-24T01:00:00Z",
  windows: {
    "30": {
      window_days: 30,
      deployments: 9,
      deployment_frequency_per_day: 0.3,
      lead_time_hours_median: 6,
      change_failure_rate: 0.111,
      time_to_restore_minutes_median: 42,
      open_criticals: 0,
    },
    "7": {
      window_days: 7,
      deployments: 3,
      deployment_frequency_per_day: 0.429,
      lead_time_hours_median: 2.5,
      change_failure_rate: 0.333,
      time_to_restore_minutes_median: 15,
      open_criticals: 1,
    },
    "90": {
      window_days: 90,
      deployments: 0,
      deployment_frequency_per_day: 0,
      lead_time_hours_median: null,
      change_failure_rate: 0,
      time_to_restore_minutes_median: null,
      open_criticals: 0,
    },
  },
};

describe("when a window is chosen", () => {
  it("should give the four keys in DORA order with readable values", () => {
    const tiles = tilesFor(json, "7");
    expect(tiles.map((t) => t.key)).toEqual([
      "deployment_frequency",
      "lead_time",
      "change_failure_rate",
      "time_to_restore",
    ]);
    expect(tiles[0]).toMatchObject({ label: "Deployment frequency", value: "0.43", unit: "/ day", note: "3 shas in 7 d" });
    expect(tiles[1]).toMatchObject({ label: "Lead time", value: "2.5", unit: "h", note: "commit to first beat, median" });
    expect(tiles[2]).toMatchObject({ label: "Change failure rate", value: "33", unit: "%", note: "critical in the first hour" });
    expect(tiles[3]).toMatchObject({ label: "Time to restore", value: "15", unit: "min", note: "critical to ok, median; 1 open" });
  });

  it("should say no data instead of a number when a key has none", () => {
    const tiles = tilesFor(json, "90");
    expect(tiles[1]).toMatchObject({ value: "no data", unit: "" });
    expect(tiles[3]).toMatchObject({ value: "no data", unit: "" });
  });
});

describe("when windows are listed", () => {
  it("should order them by days", () => {
    expect(windowsOf(json)).toEqual(["7", "30", "90"]);
  });
});
