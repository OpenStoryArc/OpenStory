import { describe, expect, it } from "vitest";
import { routeGlassKey } from "@/lib/glass-ink";

describe("routeGlassKey", () => {
  it("is null on the Draw tab — the board owns that paper", () => {
    expect(routeGlassKey({ view: "draw" })).toBeNull();
  });

  it("is null inside the reels player — beat ink owns slides", () => {
    expect(routeGlassKey({ view: "reels", reelId: "reel-1" })).toBeNull();
  });

  it("keys the reels list, live, and per-session story contexts separately", () => {
    expect(routeGlassKey({ view: "reels" })).toBe("reels");
    expect(routeGlassKey({ view: "live" })).toBe("live");
    expect(routeGlassKey({ view: "live", sessionId: "abc" })).toBe("live:abc");
    expect(routeGlassKey({ view: "story", sessionId: "abc" })).toBe("story:abc");
    expect(routeGlassKey({ view: "explore", sessionId: "s1" })).toBe("explore:s1");
  });
});
