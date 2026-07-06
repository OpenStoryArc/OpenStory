import { describe, it, expect } from "vitest";
import { replay } from "@/lib/replay";
import type { Interaction } from "@/lib/interaction";

/** The replay engine is the payoff of the agent-in-UI seam: interaction and
 *  command are INVERSES, so a captured interaction stream feeds straight back
 *  through the control seam. FORWARD retraces the journey; BACKWARD rewinds it. */
describe("replay — a captured journey becomes drivable control steps", () => {
  const nav = (view: string, sessionId?: string): Interaction =>
    ({ kind: "navigate", view, ...(sessionId ? { session_id: sessionId } : {}) });
  const sel = (session_id: string, eventId?: string): Interaction =>
    ({ kind: "select", view: "canvas", session_id, ...(eventId ? { eventId } : {}) });
  const zoom = (mode: string): Interaction => ({ kind: "zoom", view: "canvas", mode });
  const filt = (filters: Record<string, string>): Interaction =>
    ({ kind: "filter", view: "overview", filters });

  describe("when replaying forward", () => {
    it("preserves interaction order", () => {
      const steps = replay([nav("overview"), nav("canvas"), nav("story")], { direction: "forward", tempo: 1 });
      expect(steps.map((s) => s.params.view ?? s.params.route)).toEqual(["overview", "canvas", "story"]);
    });
  });

  describe("when replaying backward", () => {
    it("reverses the list to rewind the journey", () => {
      const steps = replay([nav("overview"), nav("canvas"), nav("story")], { direction: "backward", tempo: 1 });
      expect(steps.map((s) => s.params.view ?? s.params.route)).toEqual(["story", "canvas", "overview"]);
    });
  });

  describe("kind → inverse control action", () => {
    it("navigate → open_view", () => {
      const s = replay([nav("canvas", "sess-1")], { direction: "forward", tempo: 1 })[0]!;
      expect(s.action).toBe("open_view");
      expect(s.params).toMatchObject({ view: "canvas", sessionId: "sess-1" });
    });
    it("select with eventId → focus_event (the finest grain)", () => {
      const s = replay([sel("sess-1", "evt-9")], { direction: "forward", tempo: 1 })[0]!;
      expect(s.action).toBe("focus_event");
      expect(s.params).toMatchObject({ sessionId: "sess-1", eventId: "evt-9" });
    });
    it("select without eventId → open_view of the session", () => {
      const s = replay([sel("sess-1")], { direction: "forward", tempo: 1 })[0]!;
      expect(s.action).toBe("open_view");
      expect(s.params).toMatchObject({ sessionId: "sess-1" });
    });
    it("zoom → toggle canvas.mode", () => {
      const s = replay([zoom("heat")], { direction: "forward", tempo: 1 })[0]!;
      expect(s.action).toBe("toggle");
      expect(s.params).toMatchObject({ target: "canvas.mode", value: "heat" });
    });
    it("filter → query with the facet fields", () => {
      const s = replay([filt({ project: "openstory" })], { direction: "forward", tempo: 1 })[0]!;
      expect(s.action).toBe("query");
      expect(s.params).toMatchObject({ project: "openstory" });
    });
  });

  describe("tempo scales the inter-step gaps", () => {
    it("first step is at 0; gaps shrink as tempo rises", () => {
      const slow = replay([nav("a"), nav("b"), nav("c")], { direction: "forward", tempo: 1 });
      const fast = replay([nav("a"), nav("b"), nav("c")], { direction: "forward", tempo: 2 });
      expect(slow[0]!.atMs).toBe(0);
      expect(fast[0]!.atMs).toBe(0);
      // tempo 2 = twice as fast = half the gap
      expect(fast[1]!.atMs).toBe(slow[1]!.atMs / 2);
      expect(fast[2]!.atMs).toBe(slow[2]!.atMs / 2);
    });
  });

  it("skips uninterpretable kinds without throwing", () => {
    const steps = replay([{ kind: "view", view: "canvas" } as Interaction, nav("story")], { direction: "forward", tempo: 1 });
    expect(steps).toHaveLength(1);
    expect(steps[0]!.params.view).toBe("story");
  });
});
