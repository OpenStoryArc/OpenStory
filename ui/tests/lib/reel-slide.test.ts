import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  normalizeReelToSlides,
  playerToSlideIndex,
  slideInkKey,
  captionFor,
} from "@/lib/reel-slide";
import type { Reel } from "@/lib/reels-api";

const sample: Reel = {
  id: "reel-x",
  title: "Demo",
  created: "t",
  author: "a",
  opener: "BLUF line.",
  closer: "The end.",
  stops: [
    { kind: "title", line: "Body title" },
    {
      kind: "diagram",
      line: "A diagram",
      visual: { kind: "labels", labels: ["A", "B"], title: "Flow" },
    },
    {
      kind: "spotlight",
      line: "Real event",
      sessionId: "sess",
      eventId: "evt",
    },
  ],
};

describe("normalizeReelToSlides", () => {
  it("flattens opener + stops + closer into one list", () => {
    scenario(
      () => normalizeReelToSlides(sample),
      (r) => r,
      (r) => {
        expect(r.slides).toHaveLength(5);
        expect(r.slides[0]).toMatchObject({
          role: "opener",
          kind: "title",
          line: "BLUF line.",
          index: 0,
        });
        expect(r.slides[1]).toMatchObject({ role: "body", kind: "title", line: "Body title" });
        expect(r.slides[2]).toMatchObject({ kind: "diagram", line: "A diagram" });
        expect(r.slides[2]!.visual?.labels).toEqual(["A", "B"]);
        expect(r.slides[3]).toMatchObject({
          kind: "spotlight",
          anchor: { sessionId: "sess", eventId: "evt" },
        });
        expect(r.slides[4]).toMatchObject({ role: "closer", kind: "title", line: "The end." });
      },
    );
  });

  it("maps player phase to slide index for ink keys", () => {
    const { slides } = normalizeReelToSlides(sample);
    expect(playerToSlideIndex(slides, "opener", 0)).toBe(0);
    expect(playerToSlideIndex(slides, "stop", 0)).toBe(1);
    expect(playerToSlideIndex(slides, "stop", 1)).toBe(2);
    expect(playerToSlideIndex(slides, "stop", 2)).toBe(3);
    expect(playerToSlideIndex(slides, "closer", 0)).toBe(4);
    expect(slideInkKey("reel-x", 2)).toBe("reel-x:2");
  });
});

describe("captionFor", () => {
  it("suppresses the caption on title slides — the stage already shows the line", () => {
    expect(captionFor({ kind: "title", line: "Bottom line: it works." })).toBeNull();
  });

  it("returns the line for spotlight, diagram, and image slides", () => {
    expect(captionFor({ kind: "spotlight", line: "We searched first." })).toBe("We searched first.");
    expect(captionFor({ kind: "diagram", line: "The journey." })).toBe("The journey.");
    expect(captionFor({ kind: "image", line: "The screenshot." })).toBe("The screenshot.");
  });
});
