/**
 * Attention algebra — pure tree of what the mirror shows.
 */
import { describe, it, expect } from "vitest";
import {
  emptyAttention,
  foldIntent,
  foldControl,
  attentionSatisfies,
  attentionSummary,
  realizeIntent,
} from "@/lib/attention";
import { interpretControl } from "@/lib/ui-control";

describe("foldIntent — denotational navigate_to", () => {
  const base = emptyAttention({ view: "live" });

  it("event → story attention with details", () => {
    const next = foldIntent(base, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      details: true,
    });
    expect(next?.route).toMatchObject({
      view: "story",
      sessionId: "SES-1",
      eventId: "EVT-1",
      storyDetails: true,
    });
    expect(attentionSatisfies(next!, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      details: true,
    })).toBe(true);
  });

  it("expandAll opens details + eval + events", () => {
    const next = foldIntent(base, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      expandAll: true,
    });
    expect(next?.route).toMatchObject({
      storyDetails: true,
      storyEvalOpen: true,
      storyEventsOpen: true,
      storyApplyOpen: "all",
    });
  });

  it("applyOpen indices open eval-apply + per-apply outputs", () => {
    const next = foldIntent(base, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      applyOpen: [0, 2],
    });
    expect(next?.route).toMatchObject({
      view: "story",
      sessionId: "SES-1",
      eventId: "EVT-1",
      storyDetails: true,
      storyEvalOpen: true,
      storyApplyOpen: [0, 2],
    });
    expect(
      attentionSatisfies(next!, {
        kind: "event",
        id: "EVT-1",
        sessionId: "SES-1",
        applyOpen: [0, 2],
      }),
    ).toBe(true);
  });

  it("applyOpen: true / 'all' expands every apply output", () => {
    const next = foldIntent(base, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      applyOpen: true,
    });
    expect(next?.route.storyApplyOpen).toBe("all");
    expect(next?.route.storyEvalOpen).toBe(true);
  });

  it("session + gantt → canvas mode + selection", () => {
    const next = foldIntent(base, {
      kind: "session",
      id: "SES-1",
      canvasMode: "gantt",
    });
    expect(next?.route.view).toBe("canvas");
    expect(next?.canvas).toMatchObject({
      mode: "gantt",
      selectedSessionId: "SES-1",
    });
    expect(
      attentionSatisfies(next!, {
        kind: "session",
        id: "SES-1",
        canvasMode: "gantt",
      }),
    ).toBe(true);
  });

  it("canvas groupBy + metric lift into Attention", () => {
    const next = foldIntent(base, {
      kind: "canvas",
      id: "canvas",
      canvasMode: "sunburst",
      groupBy: "agent",
      metric: "tokens",
    });
    expect(next?.canvas).toMatchObject({
      mode: "sunburst",
      groupBy: "agent",
      metric: "tokens",
    });
  });

  it("foldControl toggles canvas.groupBy / metric", () => {
    let a = emptyAttention({ view: "canvas" });
    a = foldControl(a, { type: "toggle", target: "canvas.groupBy", value: "day" });
    a = foldControl(a, { type: "toggle", target: "canvas.metric", value: "tokens" });
    expect(a.canvas.groupBy).toBe("day");
    expect(a.canvas.metric).toBe("tokens");
  });

  it("property: foldIntent then attentionSatisfies for core intents", () => {
    const intents = [
      { kind: "event" as const, id: "E1", sessionId: "S1", details: true },
      { kind: "session" as const, id: "S1", canvasMode: "gantt" },
      { kind: "day" as const, id: "2026-07-27" },
      { kind: "file" as const, id: "foo.rs" },
      { kind: "person" as const, id: "max" },
      { kind: "project" as const, id: "OpenStory" },
      { kind: "reel" as const, id: "reel-abc123" },
    ];
    for (const intent of intents) {
      const next = foldIntent(base, intent);
      expect(next, JSON.stringify(intent)).not.toBeNull();
      expect(attentionSatisfies(next!, intent), JSON.stringify(intent)).toBe(true);
    }
  });

  it("day → explore filter", () => {
    const next = foldIntent(base, { kind: "day", id: "2026-07-27" });
    expect(next?.route.explore?.filters?.day).toBe("2026-07-27");
  });

  it("file → search", () => {
    const next = foldIntent(base, { kind: "file", id: "a/b/c.rs" });
    expect(next?.route).toMatchObject({
      view: "explore",
      detailView: "search",
      searchQuery: "c.rs",
    });
  });

  it("reel → reels route with reelId", () => {
    const next = foldIntent(base, { kind: "reel", id: "reel-abc123" });
    expect(next?.route).toEqual({ view: "reels", reelId: "reel-abc123" });
    expect(attentionSatisfies(next!, { kind: "reel", id: "reel-abc123" })).toBe(true);
  });

  it("reel + autoplay → reelAutoplay set on the route", () => {
    const next = foldIntent(base, { kind: "reel", id: "reel-abc123", autoplay: true });
    expect(next?.route).toEqual({
      view: "reels",
      reelId: "reel-abc123",
      reelAutoplay: true,
    });
  });

  it("spotlight event keeps separate from route", () => {
    const next = foldIntent(base, {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      spotlight: true,
    });
    expect(next?.spotlight).toEqual({
      kind: "event",
      sessionId: "SES-1",
      eventId: "EVT-1",
    });
  });
});

describe("foldControl", () => {
  it("navigate clears spotlight", () => {
    const withSpot = foldIntent(emptyAttention(), {
      kind: "event",
      id: "E",
      sessionId: "S",
      spotlight: true,
    })!;
    const next = foldControl(withSpot, {
      type: "navigate",
      route: { view: "live" },
    });
    expect(next.spotlight).toBeNull();
    expect(next.route.view).toBe("live");
  });

  it("set story.details opens details on story route", () => {
    const base = emptyAttention({
      view: "story",
      sessionId: "S",
      eventId: "E",
    });
    const next = foldControl(base, {
      type: "set",
      target: "story.details",
      params: { open: true, sessionId: "S", eventId: "E" },
    });
    expect(next.route.storyDetails).toBe(true);
  });
});

describe("realizeIntent", () => {
  it("prefers direct foldIntent", () => {
    const next = realizeIntent(
      emptyAttention(),
      { kind: "person", id: "max" },
      interpretControl,
    );
    expect(next?.route.explore?.filters?.user).toBe("max");
  });
});

describe("attentionSummary", () => {
  it("is a stable one-liner", () => {
    const a = foldIntent(emptyAttention(), {
      kind: "event",
      id: "EVT-abcdef01",
      sessionId: "SES-12345678",
      details: true,
    })!;
    expect(attentionSummary(a)).toContain("story");
    expect(attentionSummary(a)).toContain("details");
  });
});
