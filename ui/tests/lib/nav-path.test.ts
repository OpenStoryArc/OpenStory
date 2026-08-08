/**
 * Click-parity algebra tests — pure pathfinder, emitters, resolve, land.
 */
import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { ENTITY_EDGES, navigabilityReport } from "@/lib/action-graph";
import {
  shortestEntityPath,
  planNav,
  planNavigateTo,
  landMatches,
  resolveSessionFromHits,
  allReachablePairs,
  enrichContext,
  describePlan,
} from "@/lib/nav-path";

describe("shortestEntityPath", () => {
  it("event → turn is one hop", () => {
    scenario(
      () => shortestEntityPath("event", "turn"),
      (path) => path,
      (path) => {
        expect(path).toHaveLength(1);
        expect(path![0]!.via).toBe("focus_event");
      },
    );
  });

  it("person → event is multi-hop", () => {
    const path = shortestEntityPath("person", "event");
    expect(path!.map((e) => `${e.from}→${e.to}`)).toEqual([
      "person→session",
      "session→event",
    ]);
  });

  it("same kind → empty path", () => {
    expect(shortestEntityPath("session", "session")).toEqual([]);
  });
});

describe("allReachablePairs", () => {
  it("covers the full connected canopy (no orphan kinds)", () => {
    const pairs = allReachablePairs();
    expect(pairs.length).toBeGreaterThan(50);
    // every kind can reach itself (0 hops) and at least one other
    const selfs = pairs.filter((p) => p.from === p.to && p.hops === 0);
    expect(selfs.length).toBeGreaterThanOrEqual(10);
  });
});

describe("resolveSessionFromHits", () => {
  it("prefers exact event_id match", () => {
    const sid = resolveSessionFromHits("EVT-2", [
      { event_id: "EVT-1", session_id: "S1" },
      { event_id: "EVT-2", session_id: "S2" },
    ]);
    expect(sid).toBe("S2");
  });

  it("falls back to first hit", () => {
    expect(
      resolveSessionFromHits("missing", [{ event_id: "x", session_id: "S9" }]),
    ).toBe("S9");
  });

  it("returns null on empty", () => {
    expect(resolveSessionFromHits("e", [])).toBeNull();
  });
});

describe("planNav", () => {
  const ctx = {
    sessionId: "SES-1",
    eventId: "EVT-1",
    user: "max",
    project: "OpenStory",
    filePath: "rs/patterns/src/sentence.rs",
  };

  it("event → turn", () => {
    const steps = planNav(
      { kind: "event", id: "EVT-1" },
      { kind: "turn", id: "t" },
      ctx,
    );
    expect(steps![0]!.action).toBe("focus_event");
    expect(landMatches("#/story/SES-1/event/EVT-1", steps![0]!)).toBe(true);
  });

  it("toolcall → file", () => {
    const steps = planNav(
      { kind: "toolcall", id: "tc" },
      { kind: "file", id: "sentence.rs" },
      ctx,
    );
    expect(steps![0]!.params.searchQuery).toBeDefined();
  });

  it("turn → sentence expands details", () => {
    const steps = planNav(
      { kind: "turn", id: "t" },
      { kind: "sentence", id: "s" },
      ctx,
    );
    expect(steps!.some((s) => s.params.target === "story.details")).toBe(true);
  });
});

describe("planNavigateTo", () => {
  it("any event", () => {
    const steps = planNavigateTo({
      kind: "event",
      id: "EVT-9",
      sessionId: "SES-1",
      details: true,
    });
    expect(steps!.map((s) => s.action)).toEqual(["focus_event", "set"]);
  });

  // NOTE: applyOpen planning spec was committed ahead of its implementation
  // on the base branch — trimmed to committed reality; returns with the
  // apply-open feature itself.

  it("canvas gantt + session", () => {
    const steps = planNavigateTo({
      kind: "session",
      id: "SES-1",
      canvasMode: "gantt",
    });
    expect(describePlan(steps!)).toContain("canvas.mode");
    expect(steps!.some((s) => s.params.target === "canvas.select_session")).toBe(
      true,
    );
  });

  it("canvas board + expandKeys plans canvas.expand", () => {
    const steps = planNavigateTo({
      kind: "canvas",
      id: "canvas",
      canvasMode: "board",
      expandKeys: ["g:max", "p:max:OpenStory"],
    });
    const expand = steps!.find((s) => s.params.target === "canvas.expand");
    expect(expand).toMatchObject({
      action: "set",
      params: { target: "canvas.expand", keys: ["g:max", "p:max:OpenStory"] },
    });
  });

  it("heatmap day", () => {
    const steps = planNavigateTo({ kind: "day", id: "2026-07-27" });
    expect(steps![0]).toMatchObject({
      action: "query",
      params: { day: "2026-07-27" },
    });
  });

  it("event without sessionId → null (resolve first)", () => {
    expect(planNavigateTo({ kind: "event", id: "EVT-1" })).toBeNull();
  });
});

describe("enrichContext", () => {
  it("lifts ids from entity refs", () => {
    const c = enrichContext(
      {},
      { kind: "event", id: "E" },
      { kind: "session", id: "S" },
    );
    expect(c.eventId).toBe("E");
    expect(c.sessionId).toBe("S");
  });
});

describe("ENTITY_EDGES canopy", () => {
  it("has zero dead ends", () => {
    expect(navigabilityReport(ENTITY_EDGES).deadEnds).toEqual([]);
    expect(navigabilityReport(ENTITY_EDGES).coverage).toBe(1);
  });
});

describe("when navigating to a reel", () => {
  it("should plan a single open_view step to the reels tab with autoplay", () => {
    const steps = planNavigateTo({ kind: "reel", id: "reel-abc123", autoplay: true });
    expect(steps).toEqual([
      { action: "open_view", params: { view: "reels", reelId: "reel-abc123", autoplay: true } },
    ]);
  });
  it("should plan without autoplay when not requested", () => {
    const steps = planNavigateTo({ kind: "reel", id: "reel-abc123" });
    expect(steps).toEqual([
      { action: "open_view", params: { view: "reels", reelId: "reel-abc123" } },
    ]);
  });
});
