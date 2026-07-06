import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildHeatmap, heatLevel, stackBoxes } from "@/lib/heatmap";
import type { StorySession } from "@/lib/story-api";

function sess(id: string, start: string, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, start_time: start, last_event: start, origin_agent: "claude-code", status: "completed", event_count: 10, ...over };
}
// A Wednesday noon local — stable anchor for the "now" week.
const NOW = new Date(2026, 5, 24, 12, 0, 0).getTime(); // 2026-06-24

describe("buildHeatmap", () => {
  it("lays out weeks×7 cells ending in the week containing now", () => {
    scenario(
      () => buildHeatmap([], { nowMs: NOW, weeks: 4 }),
      (g) => g,
      (g) => {
        expect(g.cells).toHaveLength(4 * 7);
        expect(g.weeks).toBe(4);
        // last cell is the Saturday of the current week (>= today)
        expect(g.endDate >= "2026-06-24").toBe(true);
      },
    );
  });

  it("buckets a session into its day cell", () => {
    scenario(
      () => buildHeatmap([sess("a", "2026-06-23T09:00:00.000Z")], { nowMs: NOW, weeks: 4 }),
      (g) => g.cells.find((c) => c.count > 0),
      (cell) => {
        expect(cell).toBeTruthy();
        expect(cell!.sessions[0]!.id).toBe("a");
      },
    );
  });

  it("sorts a day's sessions largest → smallest (stack base = biggest)", () => {
    scenario(
      () => buildHeatmap([
        sess("small", "2026-06-22T09:00:00.000Z", { event_count: 5 }),
        sess("big", "2026-06-22T10:00:00.000Z", { event_count: 900 }),
        sess("mid", "2026-06-22T11:00:00.000Z", { event_count: 100 }),
      ], { nowMs: NOW, weeks: 4 }),
      (g) => g.cells.find((c) => c.date === "2026-06-22")!,
      (cell) => {
        expect(cell.sessions.map((s) => s.id)).toEqual(["big", "mid", "small"]);
        expect(cell.count).toBe(3);
      },
    );
  });

  it("excludes sessions outside the window and tracks maxCount", () => {
    scenario(
      () => buildHeatmap([
        sess("in", "2026-06-23T09:00:00.000Z"),
        sess("ancient", "2020-01-01T09:00:00.000Z"),
      ], { nowMs: NOW, weeks: 4 }),
      (g) => g,
      (g) => {
        expect(g.totalSessions).toBe(1); // ancient dropped
        expect(g.maxCount).toBe(1);
      },
    );
  });

  it("flags future padding cells as not present", () => {
    scenario(
      () => buildHeatmap([], { nowMs: NOW, weeks: 4 }),
      (g) => g.cells.filter((c) => !c.present).length,
      (futureCount) => {
        // current week has days after Wednesday → some future cells exist
        expect(futureCount).toBeGreaterThan(0);
      },
    );
  });
});

describe("heatLevel", () => {
  it("maps 0 → level 0 and the max → level 4", () => {
    expect(heatLevel(0, 10)).toBe(0);
    expect(heatLevel(10, 10)).toBe(4);
  });
  it("buckets intermediate counts into 1..3", () => {
    expect(heatLevel(2, 10)).toBe(1);
    expect(heatLevel(4, 10)).toBe(2);
    expect(heatLevel(7, 10)).toBe(3);
  });
});

describe("stackBoxes — the 3D stack fold (one box per session, explorable)", () => {
  const NOW = new Date("2026-06-12T12:00:00.000Z").getTime();
  const sess = (id: string, events: number) => ({
    session_id: id,
    start_time: "2026-06-10T09:00:00.000Z",
    last_event: "2026-06-10T10:00:00.000Z",
    event_count: events,
    label: `label-${id}`,
  });

  it("should give every box its session so hover/click can reach it", () => {
    const grid = buildHeatmap([sess("big", 100), sess("small", 5)], { nowMs: NOW, weeks: 2 });
    const boxes = stackBoxes(grid, 12);
    expect(boxes).toHaveLength(2);
    // base box = the biggest session of the day
    expect(boxes[0]!.session?.id).toBe("big");
    expect(boxes[0]!.level).toBe(0);
    expect(boxes[1]!.session?.id).toBe("small");
    expect(boxes[1]!.overflow).toBe(false);
  });

  it("should cap tall days with an overflow box that carries NO single session", () => {
    const many = Array.from({ length: 15 }, (_, i) => sess(`s${i}`, 10 + i));
    const grid = buildHeatmap(many, { nowMs: NOW, weeks: 2 });
    const boxes = stackBoxes(grid, 12);
    expect(boxes).toHaveLength(12); // capped
    const top = boxes[boxes.length - 1]!;
    expect(top.overflow).toBe(true);
    expect(top.session).toBeUndefined();
    expect(top.hidden).toBe(4); // 15 sessions, 11 real boxes + cap
  });
});
