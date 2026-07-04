import { describe, it, expect } from "vitest";
import { ENTITY_EDGES, navigabilityReport, type DataEdge } from "@/lib/action-graph";

/** The navigation graph as DATA: every relationship the data has is an edge a
 *  person should be able to walk. An edge is "realized" when a control verb walks
 *  it; a DEAD END is an edge the data has but the UI never grew. The report is
 *  the testable heart — dead ends are a number, not a vibe. */
describe("navigabilityReport — which data edges are walkable", () => {
  it("splits edges into realized (has a verb) vs dead ends (via null)", () => {
    const edges: DataEdge[] = [
      { from: "turn", to: "event", label: "source", via: "focus_event" },
      { from: "event", to: "turn", label: "its turn ↑", via: null },
      { from: "turn", to: "sentence", label: "summarized", via: "inherent" },
    ];
    const r = navigabilityReport(edges);
    expect(r.total).toBe(3);
    expect(r.realized.map((e) => e.label)).toEqual(["source", "summarized"]);
    expect(r.deadEnds.map((e) => e.label)).toEqual(["its turn ↑"]);
    expect(r.drivable.map((e) => e.label)).toEqual(["source"]); // inherent isn't driven
    expect(r.coverage).toBeCloseTo(2 / 3);
  });

  it("empty graph → zero coverage, no crash", () => {
    const r = navigabilityReport([]);
    expect(r).toMatchObject({ total: 0, coverage: 0 });
    expect(r.deadEnds).toEqual([]);
  });
});

describe("ENTITY_EDGES — the real data model", () => {
  it("includes the turn→event edge we just closed (via focus_event)", () => {
    const e = ENTITY_EDGES.find((x) => x.from === "turn" && x.to === "event");
    expect(e?.via).toBe("focus_event");
  });

  it("includes the event→turn edge — the climb back up (via focus_event)", () => {
    // Both directions now walk: a turn drills to its source event, and an
    // event climbs to its turn in Story (#/story/SES/event/ID).
    const e = ENTITY_EDGES.find((x) => x.from === "event" && x.to === "turn");
    expect(e?.via).toBe("focus_event");
  });

  it("includes the file→session edge — impact across sessions (via query)", () => {
    const e = ENTITY_EDGES.find((x) => x.from === "file" && x.to === "session");
    expect(e?.via).toBe("query");
  });

  it("includes the subagent→session edge — climb to the spawner (via open_view)", () => {
    const e = ENTITY_EDGES.find((x) => x.from === "subagent" && x.to === "session");
    expect(e?.via).toBe("open_view");
  });

  it("still has the known dead ends (data connected, UI isn't)", () => {
    const report = navigabilityReport(ENTITY_EDGES);
    const deadLabels = new Set(report.deadEnds.map((e) => `${e.from}->${e.to}`));
    // the branches that stop in mid-air, from the picture
    expect(deadLabels.has("plan->turn")).toBe(true);
    expect(deadLabels.has("toolcall->result")).toBe(true);
    expect(deadLabels.has("error->event")).toBe(true);
  });

  it("reports partial coverage — the canopy is half-grown", () => {
    const r = navigabilityReport(ENTITY_EDGES);
    expect(r.total).toBeGreaterThan(10);
    expect(r.coverage).toBeGreaterThan(0.3);
    expect(r.coverage).toBeLessThan(0.8);
  });
});
