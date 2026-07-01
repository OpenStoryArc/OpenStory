import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildGantt } from "@/lib/sessions-gantt";
import type { StorySession } from "@/lib/story-api";

function sess(id: string, start: string, end: string | null, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, start_time: start, last_event: end, user: "max", origin_agent: "claude-code", status: end ? "completed" : "ongoing", ...over };
}
const T = (h: number, m = 0) => `2026-06-30T${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:00.000Z`;
const NOW = Date.parse("2026-06-30T12:00:00.000Z");

describe("buildGantt", () => {
  it("packs non-overlapping sessions into the same lane", () => {
    scenario(
      () => buildGantt([sess("a", T(9), T(10)), sess("b", T(10, 1), T(11))], "user", NOW),
      (m) => m.bars.map((b) => [b.id, b.lane]),
      (r) => {
        expect(r).toContainEqual(["a", 0]);
        expect(r).toContainEqual(["b", 0]); // b starts after a ends → same lane
      },
    );
  });

  it("puts overlapping sessions in separate lanes", () => {
    scenario(
      () => buildGantt([sess("a", T(9), T(11)), sess("b", T(10), T(12))], "user", NOW),
      (m) => new Set(m.bars.map((b) => b.lane)).size,
      (laneCount) => expect(laneCount).toBe(2),
    );
  });

  it("ends an ongoing session at nowMs", () => {
    scenario(
      () => buildGantt([sess("live", T(11), null, { status: "ongoing" })], "user", NOW),
      (m) => m.bars[0]!,
      (bar) => {
        expect(bar.ongoing).toBe(true);
        expect(bar.endMs).toBe(NOW);
      },
    );
  });

  it("bands by the group dimension with lane offsets", () => {
    scenario(
      () => buildGantt(
        [sess("m1", T(9), T(11), { user: "max" }), sess("m2", T(10), T(12), { user: "max" }), sess("k1", T(9), T(10), { user: "katie" })],
        "user", NOW,
      ),
      (m) => m,
      (m) => {
        const max = m.bands.find((b) => b.name === "max")!;
        const katie = m.bands.find((b) => b.name === "katie")!;
        expect(max.laneCount).toBe(2); // m1,m2 overlap
        expect(katie.laneStart).toBeGreaterThanOrEqual(max.laneStart + max.laneCount);
        // katie's bar sits in her band's lane range
        const k = m.bars.find((b) => b.id === "k1")!;
        expect(k.lane).toBeGreaterThanOrEqual(katie.laneStart);
      },
    );
  });

  it("computes the time domain across all bars", () => {
    scenario(
      () => buildGantt([sess("a", T(9), T(10)), sess("b", T(11), T(13))], "user", NOW),
      (m) => m.domain,
      (d) => {
        expect(d[0]).toBe(Date.parse(T(9)));
        expect(d[1]).toBe(Date.parse(T(13)));
      },
    );
  });
});
