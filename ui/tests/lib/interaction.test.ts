import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { interactionFromRoute } from "@/lib/interaction";

describe("interactionFromRoute", () => {
  it("maps a simple view route to a navigate interaction", () => {
    scenario(
      () => interactionFromRoute({ view: "heatmap" }),
      (p) => p,
      (p) => {
        expect(p.kind).toBe("navigate");
        expect(p.view).toBe("heatmap");
        expect(p.session_id).toBeUndefined();
      },
    );
  });

  it("carries session + detail + event ids", () => {
    scenario(
      () => interactionFromRoute({ view: "explore", sessionId: "s1", detailView: "conversation", eventId: "e9" }),
      (p) => p,
      (p) => {
        expect(p.session_id).toBe("s1");
        expect(p.detailView).toBe("conversation");
        expect(p.eventId).toBe("e9");
      },
    );
  });

  it("includes Explore filters when present, omits when empty", () => {
    scenario(
      () => ({
        withF: interactionFromRoute({ view: "explore", explore: { filters: { agent: "pi-mono" } } }),
        empty: interactionFromRoute({ view: "explore", explore: { filters: {} } }),
      }),
      (r) => r,
      (r) => {
        expect(r.withF.filters).toEqual({ agent: "pi-mono" });
        expect(r.empty.filters).toBeUndefined();
      },
    );
  });
});
