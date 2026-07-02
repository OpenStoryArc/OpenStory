import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildStreamgraph } from "@/lib/events-streamgraph";

const S = (agent: string, day: string, events: number) => ({ origin_agent: agent, start_time: `${day}T12:00:00Z`, event_count: events });

describe("buildStreamgraph", () => {
  it("buckets event_count by day and agent", () => {
    scenario(
      () => buildStreamgraph([S("claude-code", "2026-06-01", 10), S("claude-code", "2026-06-01", 5), S("pi-mono", "2026-06-01", 3)]),
      (g) => g.rows.find((r) => r.day === "2026-06-01")!,
      (row) => {
        expect(row["claude-code"]).toBe(15);
        expect(row["pi-mono"]).toBe(3);
      },
    );
  });

  it("ranks agents by total events descending", () => {
    scenario(
      () => buildStreamgraph([S("small", "2026-06-01", 2), S("big", "2026-06-01", 100), S("mid", "2026-06-01", 40)]),
      (g) => g.agents,
      (agents) => expect(agents).toEqual(["big", "mid", "small"]),
    );
  });

  it("fills gaps so days are contiguous with zero-total rows", () => {
    scenario(
      () => buildStreamgraph([S("a", "2026-06-01", 5), S("a", "2026-06-04", 5)]),
      (g) => g.days,
      (days) => expect(days).toEqual(["2026-06-01", "2026-06-02", "2026-06-03", "2026-06-04"]),
    );
  });

  it("gives every row a value for every agent (zero when absent)", () => {
    scenario(
      () => buildStreamgraph([S("a", "2026-06-01", 5), S("b", "2026-06-02", 7)]),
      (g) => g,
      (g) => {
        for (const r of g.rows) for (const a of g.agents) expect(typeof r[a]).toBe("number");
        expect(g.rows.find((r) => r.day === "2026-06-01")!["b"]).toBe(0);
      },
    );
  });

  it("folds agents beyond topAgents into 'other'", () => {
    scenario(
      () => buildStreamgraph([S("a", "2026-06-01", 100), S("b", "2026-06-01", 50), S("c", "2026-06-01", 10), S("d", "2026-06-01", 5)], { topAgents: 2 }),
      (g) => g,
      (g) => {
        expect(g.agents).toEqual(["a", "b", "other"]);
        expect(g.rows[0]!["other"]).toBe(15); // c + d
      },
    );
  });

  it("returns empty structure for no dated sessions", () => {
    scenario(
      () => buildStreamgraph([{ origin_agent: "a", event_count: 5 }]),
      (g) => g,
      (g) => { expect(g.days).toEqual([]); expect(g.agents).toEqual([]); expect(g.rows).toEqual([]); },
    );
  });
});
