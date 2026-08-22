import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { interactionFromRoute } from "@/lib/interaction";

describe("interactionFromRoute", () => {
  it("maps a simple view route to a navigate interaction", () => {
    scenario(
      () => interactionFromRoute({ view: "canvas" }),
      (p) => p,
      (p) => {
        expect(p.kind).toBe("navigate");
        expect(p.view).toBe("canvas");
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

  it("carries reelId for reels player journeys", () => {
    scenario(
      () => interactionFromRoute({ view: "reels", reelId: "reel-abc" }),
      (p) => p,
      (p) => {
        expect(p.view).toBe("reels");
        expect(p.reelId).toBe("reel-abc");
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

describe("when route carries full bookmarkable state", () => {
  it("should report filePath, userFilter, timeFilter, and searchQuery for where_is_user parity", () => {
    scenario(
      () =>
        interactionFromRoute({
          view: "explore",
          sessionId: "s1",
          filePath: "src/a.ts",
          detailView: "events",
          searchQuery: "auth",
          userFilter: "katie",
          timeFilter: "today",
          explore: { filters: { agent: "grok" } },
        }),
      (p) => p,
      (p) => {
        expect(p.kind).toBe("navigate");
        expect(p.session_id).toBe("s1");
        expect(p.filePath).toBe("src/a.ts");
        expect(p.detailView).toBe("events");
        expect(p.searchQuery).toBe("auth");
        expect(p.userFilter).toBe("katie");
        expect(p.timeFilter).toBe("today");
        expect(p.filters).toEqual({ agent: "grok" });
      },
    );
  });
});
