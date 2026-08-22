import { describe, it, expect } from "vitest";
import {
  filterInteraction,
  glassInkInteraction,
  selectInteraction,
  zoomInteraction,
} from "@/lib/interaction";

describe("interaction builders — click-granular capture", () => {
  it("selectInteraction carries view + session, optionally an event", () => {
    expect(selectInteraction("canvas", "s1")).toEqual({ kind: "select", view: "canvas", session_id: "s1" });
    expect(selectInteraction("explore", "s1", "e9")).toEqual({ kind: "select", view: "explore", session_id: "s1", eventId: "e9" });
  });
  it("filterInteraction carries the filter object", () => {
    expect(filterInteraction("overview", { project: "OpenStory" })).toEqual({ kind: "filter", view: "overview", filters: { project: "OpenStory" } });
  });
  it("zoomInteraction carries optional mode/zoom", () => {
    expect(zoomInteraction("heatmap", { zoom: 52 })).toEqual({ kind: "zoom", view: "heatmap", zoom: 52 });
    expect(zoomInteraction("canvas", { mode: "sunburst" })).toEqual({ kind: "zoom", view: "canvas", mode: "sunburst" });
  });
});

describe("glassInkInteraction — glass strokes keep the route's context", () => {
  it("preserves session + detail context alongside the ink snapshot", () => {
    expect(
      glassInkInteraction(
        { view: "story", sessionId: "s1", detailView: "events" },
        "story:s1",
        3,
      ),
    ).toEqual({
      kind: "navigate",
      view: "story",
      session_id: "s1",
      detailView: "events",
      glassInk: { key: "story:s1", stroke_count: 3 },
    });
  });

  it("carries a bare view context when there is no session", () => {
    expect(glassInkInteraction({ view: "live" }, "live", 1)).toEqual({
      kind: "navigate",
      view: "live",
      glassInk: { key: "live", stroke_count: 1 },
    });
  });
});
