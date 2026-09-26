/** ReelBeatStage — non-spotlight beats (title / diagram / image).
 *
 *  The player's caption bar is a fixed, bottom-anchored band layered above
 *  the stage. A beat that centers itself in the full viewport puts up to a
 *  quarter of its picture under that band (seen on the first illustrated
 *  arc reel, 2026-09-23: the bottom of every chart was hidden). The stage
 *  therefore takes the measured band height and keeps every beat above it. */

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { ReelBeatStage } from "@/components/reels/ReelBeatStage";
import type { ReelStop } from "@/lib/reels-api";

afterEach(() => cleanup());

const imageStop: ReelStop = {
  sessionId: "",
  eventId: "",
  kind: "image",
  line: "Commits on master, by week",
  visual: { kind: "image", imageHref: "data:image/png;base64,iVBORw0KGgo=" },
};

describe("when an image beat plays under a caption band", () => {
  it("should reserve the band's height so the picture stays above it", () => {
    render(<ReelBeatStage stop={imageStop} onClose={vi.fn()} reservedBottom={212} />);
    const stage = screen.getByTestId("reel-beat-image");
    expect(stage.style.paddingBottom).toBe("212px");
  });

  it("should fit the picture inside the remaining area without cropping", () => {
    render(<ReelBeatStage stop={imageStop} onClose={vi.fn()} reservedBottom={212} />);
    const img = screen.getByRole("img", { name: "Commits on master, by week" });
    expect(img.className).toContain("object-contain");
    expect(img.className).toContain("max-h-full");
    expect(img.className).toContain("max-w-full");
  });
});

describe("when no caption band height is given", () => {
  it("should reserve nothing", () => {
    render(<ReelBeatStage stop={imageStop} onClose={vi.fn()} />);
    expect(screen.getByTestId("reel-beat-image").style.paddingBottom).toBe("0px");
  });
});

describe("when a title beat plays under a caption band", () => {
  it("should reserve the band's height too", () => {
    const titleStop: ReelStop = { sessionId: "", eventId: "", kind: "title", line: "BLUF" };
    render(<ReelBeatStage stop={titleStop} onClose={vi.fn()} reservedBottom={100} />);
    expect(screen.getByTestId("reel-beat-title").style.paddingBottom).toBe("100px");
  });
});
