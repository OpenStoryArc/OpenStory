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

import {
  appendGlassInk,
  applyGlassInkIntent,
  clearGlassInk,
  emptyGlassInkStore,
  getGlassInk,
  normalizeGlassInkStore,
  pruneGlassInkStore,
} from "@/lib/glass-ink";

const stroke = { type: "line", x1: 0, y1: 0, x2: 1, y2: 1 } as const;

describe("glass ink store", () => {
  it("appends strokes under one context without touching others", () => {
    let s = emptyGlassInkStore();
    s = appendGlassInk(s, "live", [stroke], { now: () => "t1" });
    s = appendGlassInk(s, "story:abc", [stroke, stroke], { now: () => "t2" });
    expect(getGlassInk(s, "live").strokes.length).toBe(1);
    expect(getGlassInk(s, "story:abc").strokes.length).toBe(2);
    expect(getGlassInk(s, "explore").strokes.length).toBe(0);
  });

  it("clear empties exactly one context", () => {
    let s = emptyGlassInkStore();
    s = appendGlassInk(s, "live", [stroke]);
    s = appendGlassInk(s, "reels", [stroke]);
    s = clearGlassInk(s, "live");
    expect(getGlassInk(s, "live").strokes.length).toBe(0);
    expect(getGlassInk(s, "reels").strokes.length).toBe(1);
  });

  it("replace intent overwrites; append (default) accumulates", () => {
    let s = emptyGlassInkStore();
    s = applyGlassInkIntent(s, { key: "live", strokes: [stroke, stroke] });
    s = applyGlassInkIntent(s, { key: "live", strokes: [stroke], mode: "replace" });
    expect(getGlassInk(s, "live").strokes.length).toBe(1);
  });

  it("prunes to the most recently updated contexts", () => {
    let s = emptyGlassInkStore();
    s = appendGlassInk(s, "a", [stroke], { now: () => "2026-01-01" });
    s = appendGlassInk(s, "b", [stroke], { now: () => "2026-01-02" });
    s = appendGlassInk(s, "c", [stroke], { now: () => "2026-01-03" });
    const pruned = pruneGlassInkStore(s, 2);
    expect(Object.keys(pruned.byKey).sort()).toEqual(["b", "c"]);
  });

  it("normalizes untrusted storage JSON (bad rows dropped, versions gated)", () => {
    expect(normalizeGlassInkStore(null).byKey).toEqual({});
    expect(normalizeGlassInkStore({ v: 99, byKey: {} }).byKey).toEqual({});
    const ok = normalizeGlassInkStore({
      v: 1,
      byKey: { live: { strokes: [stroke], updatedAt: "t" }, bad: 7 },
    });
    expect(Object.keys(ok.byKey)).toEqual(["live"]);
  });
});
