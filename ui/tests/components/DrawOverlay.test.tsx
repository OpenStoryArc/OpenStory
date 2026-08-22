import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { DrawOverlay } from "@/components/draw/DrawOverlay";
import { clearGlassContext } from "@/streams/glass-ink";
import { setDrawInteractive } from "@/streams/draw";
import type { HashRoute } from "@/lib/hash-route";

/** jsdom has no PointerEvent, and the overlay only listens for pointer events.
 *  A MouseEvent subclass carrying pointerId is enough to drive a real stroke. */
class TestPointerEvent extends MouseEvent {
  readonly pointerId: number;
  constructor(type: string, props: MouseEventInit & { pointerId?: number } = {}) {
    super(type, props);
    this.pointerId = props.pointerId ?? 1;
  }
}
if (!("PointerEvent" in globalThis)) {
  (globalThis as unknown as { PointerEvent: unknown }).PointerEvent = TestPointerEvent;
}

/** Story detail route — the exact shape Finding 1 was losing. */
const STORY_ROUTE: HashRoute = { view: "story", sessionId: "s1", detailView: "events" };
const STORY_KEY = "story:s1";

function stubGlassRect(): void {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    left: 0,
    top: 0,
    width: 1000,
    height: 1000,
    right: 1000,
    bottom: 1000,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
}

/** Drag a two-point stroke across the overlay's SVG. */
function drawStroke(): void {
  const svg = screen.getByTestId("draw-overlay").querySelector("svg")!;
  fireEvent.pointerDown(svg, { clientX: 100, clientY: 100, pointerId: 1 });
  fireEvent.pointerMove(svg, { clientX: 400, clientY: 400, pointerId: 1 });
  fireEvent.pointerUp(svg, { clientX: 400, clientY: 400, pointerId: 1 });
}

describe("DrawOverlay — human glass strokes report to agent eyes", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    clearGlassContext(STORY_KEY);
    setDrawInteractive(false);
    fetchSpy = vi.fn(() => Promise.resolve(new Response("{}")));
    vi.stubGlobal("fetch", fetchSpy);
    stubGlassRect();
  });

  afterEach(() => {
    setDrawInteractive(false);
    clearGlassContext(STORY_KEY);
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("posts an interaction that keeps the route's session + detail context", () => {
    // Annotate mode before mount: the overlay reads drawInteractive$ on
    // subscribe, so the surface is live on first paint.
    setDrawInteractive(true);
    const { unmount } = render(<DrawOverlay route={STORY_ROUTE} />);
    drawStroke();

    const bodies = fetchSpy.mock.calls
      .filter(([url]) => String(url).includes("/api/interactions"))
      .map(([, init]) => JSON.parse(String((init as RequestInit).body)));
    const inked = bodies.filter((b) => b.glassInk);
    expect(inked).toHaveLength(1);
    expect(inked[0]).toMatchObject({
      kind: "navigate",
      view: "story",
      session_id: "s1",
      detailView: "events",
      glassInk: { key: STORY_KEY, stroke_count: expect.any(Number) },
    });
    expect(inked[0].glassInk.stroke_count).toBeGreaterThan(0);
    unmount();
  });
});
