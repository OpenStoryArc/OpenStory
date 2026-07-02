import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildParallelCoords } from "@/lib/parallel-coords";

const S = (id: string, agent: string, events: number, tin: number, tout: number, start: string, end: string) => ({
  session_id: id, origin_agent: agent, event_count: events, total_input_tokens: tin, total_output_tokens: tout, start_time: start, last_event: end,
});

// two sessions, events 10 & 30 → min/max on the events axis
const A = S("a", "claude-code", 10, 100, 5, "2026-06-01T00:00:00Z", "2026-06-01T00:10:00Z");
const B = S("b", "pi-mono", 30, 300, 25, "2026-06-01T00:00:00Z", "2026-06-01T00:30:00Z");

describe("buildParallelCoords", () => {
  it("normalizes each axis independently to [0,1] (min→0, max→1)", () => {
    scenario(
      () => buildParallelCoords([A, B]),
      (pc) => pc.lines,
      (lines) => {
        const la = lines.find((l) => l.session_id === "a")!;
        const lb = lines.find((l) => l.session_id === "b")!;
        expect(la.coords[0]).toBeCloseTo(0); // min events
        expect(lb.coords[0]).toBeCloseTo(1); // max events
      },
    );
  });

  it("maps a constant axis to 0.5 (no divide-by-zero)", () => {
    scenario(
      () => buildParallelCoords([
        S("a", "x", 5, 5, 5, "2026-06-01T00:00:00Z", "2026-06-01T00:05:00Z"),
        S("b", "x", 5, 5, 5, "2026-06-01T00:00:00Z", "2026-06-01T00:05:00Z"),
      ]),
      (pc) => pc.lines,
      (lines) => lines.forEach((l) => l.coords.forEach((c) => expect(c).toBeCloseTo(0.5))),
    );
  });

  it("carries session_id, agent and raw values in axis order", () => {
    scenario(
      () => buildParallelCoords([A, B]),
      (pc) => ({ line: pc.lines.find((l) => l.session_id === "a")!, axes: pc.axes }),
      ({ line, axes }) => {
        expect(line.agent).toBe("claude-code");
        expect(axes.map((x) => x.key)).toEqual(["events", "in", "out", "duration"]);
        expect(line.raw[0]).toBe(10); // events
        expect(line.raw[1]).toBe(100); // in-tokens
      },
    );
  });

  it("produces one line per session", () => {
    scenario(
      () => buildParallelCoords([A, B]),
      (pc) => pc.lines.length,
      (n) => expect(n).toBe(2),
    );
  });

  it("reports per-axis domains as [min,max]", () => {
    scenario(
      () => buildParallelCoords([A, B]),
      (pc) => pc.domains,
      (domains) => { expect(domains[0]).toEqual([10, 30]); expect(domains[1]).toEqual([100, 300]); },
    );
  });

  it("returns empty structure for no sessions", () => {
    scenario(
      () => buildParallelCoords([]),
      (pc) => pc,
      (pc) => { expect(pc.lines).toEqual([]); expect(pc.axes.length).toBe(4); },
    );
  });
});
