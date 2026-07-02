import { describe, it, expect } from "vitest";
import { selectInteraction, filterInteraction, zoomInteraction } from "@/lib/interaction";

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
