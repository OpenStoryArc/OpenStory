import { describe, it, expect } from "vitest";
import { STICKIES, FLOWS, JOURNEYS, neighborsOf, journeyEdges, stickyById } from "@/lib/event-storming";

/** The Event Storming board as data — actors, commands, aggregates, events,
 *  policies, read models — and the flows between them. Interactivity is graph
 *  traversal over this model: click a sticky → light its neighbors; pick a
 *  journey → light its path. Pure, so it's all testable. */
describe("neighborsOf — a sticky's upstream + downstream (for click-highlight)", () => {
  const flows = [
    { from: "a", to: "b" },
    { from: "b", to: "c" },
    { from: "x", to: "b" },
  ];
  it("returns what points INTO and OUT OF a node", () => {
    const n = neighborsOf(flows, "b");
    expect(new Set(n.upstream)).toEqual(new Set(["a", "x"]));
    expect(n.downstream).toEqual(["c"]);
  });
  it("isolated node → empty both ways", () => {
    expect(neighborsOf(flows, "z")).toEqual({ upstream: [], downstream: [] });
  });
});

describe("journeyEdges — a journey path becomes consecutive flow edges", () => {
  it("turns [a,b,c] into a→b, b→c", () => {
    expect(journeyEdges({ id: "j", name: "J", path: ["a", "b", "c"] })).toEqual([
      { from: "a", to: "b" },
      { from: "b", to: "c" },
    ]);
  });
  it("single-node journey has no edges", () => {
    expect(journeyEdges({ id: "j", name: "J", path: ["a"] })).toEqual([]);
  });
});

describe("the OpenStory board is well-formed", () => {
  it("every flow references real stickies", () => {
    const ids = new Set(STICKIES.map((s) => s.id));
    for (const f of FLOWS) {
      expect(ids.has(f.from), `flow.from ${f.from}`).toBe(true);
      expect(ids.has(f.to), `flow.to ${f.to}`).toBe(true);
    }
  });
  it("every journey path references real stickies", () => {
    const ids = new Set(STICKIES.map((s) => s.id));
    for (const j of JOURNEYS) for (const id of j.path) expect(ids.has(id), `${j.id}:${id}`).toBe(true);
  });
  it("covers all seven Event Storming sticky kinds", () => {
    const kinds = new Set(STICKIES.map((s) => s.kind));
    for (const k of ["actor", "command", "aggregate", "event", "policy", "readmodel", "external"])
      expect(kinds.has(k as never), k).toBe(true);
  });
  it("spans both bounded contexts", () => {
    const ctx = new Set(STICKIES.map((s) => s.context));
    expect(ctx.has("observed")).toBe(true);
    expect(ctx.has("authored")).toBe(true);
  });
  it("stickyById resolves a known sticky", () => {
    expect(stickyById("session")?.kind).toBe("aggregate");
  });
});
