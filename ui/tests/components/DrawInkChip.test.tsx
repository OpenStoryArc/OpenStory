import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { DrawInkChip } from "@/components/draw/DrawInkChip";
import { appendGlassStrokes, clearGlassContext } from "@/streams/glass-ink";
import type { HashRoute } from "@/lib/hash-route";

const STORY_ROUTE: HashRoute = { view: "story", sessionId: "s1" };
const STORY_KEY = "story:s1";

describe("DrawInkChip", () => {
  // Reset the shared glass-ink store before each test, with no component
  // mounted at the time — avoids updating state on a stale-but-still-mounted
  // instance from the previous test outside of act().
  beforeEach(() => {
    clearGlassContext(STORY_KEY);
  });

  it("renders nothing on the Draw tab (no glass context)", () => {
    const { unmount } = render(<DrawInkChip route={{ view: "draw" }} onOpenDraw={() => {}} />);
    expect(screen.queryByTestId("draw-ink-chip")).toBeNull();
    unmount();
  });

  it("renders nothing on the reels player (BeatInkLayer owns ink there)", () => {
    const { unmount } = render(
      <DrawInkChip route={{ view: "reels", reelId: "r1" }} onOpenDraw={() => {}} />,
    );
    expect(screen.queryByTestId("draw-ink-chip")).toBeNull();
    unmount();
  });

  it("shows a zero count with no Clear button when the context has no ink", () => {
    const { unmount } = render(<DrawInkChip route={STORY_ROUTE} onOpenDraw={() => {}} />);
    const chip = screen.getByTestId("draw-ink-chip");
    expect(chip.textContent).toContain("0 here");
    expect(screen.queryByText("Clear")).toBeNull();
    unmount();
  });

  it("counts only the current context's strokes, labeled 'here'", () => {
    appendGlassStrokes(STORY_KEY, [{ type: "path", points: [{ x: 0.1, y: 0.1 }, { x: 0.2, y: 0.2 }] }]);
    const { unmount } = render(<DrawInkChip route={STORY_ROUTE} onOpenDraw={() => {}} />);
    expect(screen.getByTestId("draw-ink-chip").textContent).toContain("1 here");
    unmount();
  });

  it("Clear removes ink from this view's context only, titled to say so", () => {
    appendGlassStrokes(STORY_KEY, [{ type: "path", points: [{ x: 0.1, y: 0.1 }, { x: 0.2, y: 0.2 }] }]);
    const { unmount } = render(<DrawInkChip route={STORY_ROUTE} onOpenDraw={() => {}} />);
    const clearBtn = screen.getByTitle("Clear ink on this view");
    fireEvent.click(clearBtn);
    expect(screen.getByTestId("draw-ink-chip").textContent).toContain("0 here");
    unmount();
  });

  it("labels the board-opening button 'Board' and calls onOpenDraw", () => {
    const onOpenDraw = vi.fn();
    const { unmount } = render(<DrawInkChip route={STORY_ROUTE} onOpenDraw={onOpenDraw} />);
    const boardBtn = screen.getByText("Board");
    fireEvent.click(boardBtn);
    expect(onOpenDraw).toHaveBeenCalledOnce();
    unmount();
  });
});
