/** ReelsView — list + spotlight player.
 *
 *  Covers the click-to-advance contract fixed after code review: a click on
 *  the reel playback surface must ADVANCE, not fall through to
 *  EventSpotlight/TitleSpotlight's own backdrop `onClick={onClose}` (which
 *  would exit the reel instead). Esc still exits. See the
 *  `PlaybackClickSurface` doc comment in ReelsView.tsx for the trap this
 *  guards against. */

import { describe, it, expect, vi, afterEach } from "vitest";
import { act, render, screen, waitFor, fireEvent } from "@testing-library/react";
import { ReelsView } from "@/components/reels/ReelsView";
import type { HashRoute } from "@/lib/hash-route";
import type { Reel, ReelMeta } from "@/lib/reels-api";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

/** Minimal, non-firing speechSynthesis stub. The narration effect takes the
 *  Web Speech branch when `"speechSynthesis" in window`, but this stub's
 *  `speak()` never calls the utterance's `onend` — so no fallback timer is
 *  ever scheduled and no auto-ADVANCE can race a test's assertions. This is
 *  what makes the narration path deterministic under test: real speech
 *  synthesis (and its variable timing) never runs, and the caption-paced
 *  fallback branch is never taken either, so playback only ever advances
 *  from an explicit click, Space, or Esc in test code. */
function stubSpeechSynthesis() {
  vi.stubGlobal("speechSynthesis", { cancel: vi.fn(), speak: vi.fn() });
  vi.stubGlobal(
    "SpeechSynthesisUtterance",
    class {
      rate = 1;
      onend: (() => void) | null = null;
      constructor(public text: string) {}
    },
  );
}

/** Simulates the Chrome TTS quirk behind FINDING 2: `cancel()` doesn't
 *  invoke the outgoing utterance's `onend` synchronously within the call
 *  — it's the browser that fires it, as a queued task that lands AFTER
 *  the current synchronous work (including React's effect cleanup) has
 *  already returned. A plain `vi.fn()` can't reproduce that ordering, so
 *  `cancel()` here defers the callback via `setTimeout(..., 0)`, which
 *  only runs once the test explicitly advances fake timers — letting the
 *  spec prove the late callback lands on a *dead* effect closure. */
function stubSpeechSynthesisCancelFiresOnendLate() {
  let current: { onend: (() => void) | null } | null = null;
  vi.stubGlobal("speechSynthesis", {
    speak: vi.fn((u: { onend: (() => void) | null }) => {
      current = u;
    }),
    cancel: vi.fn(() => {
      const outgoing = current;
      current = null;
      if (outgoing) setTimeout(() => outgoing.onend?.(), 0);
    }),
  });
  vi.stubGlobal(
    "SpeechSynthesisUtterance",
    class {
      rate = 1;
      onend: (() => void) | null = null;
      constructor(public text: string) {}
    },
  );
}

/** Routes /api/reels, /api/reels/{id}, and (for EventSpotlight, which the
 *  player mounts per stop) /api/sessions/{id}/conversation. The conversation
 *  endpoint 404s by default — these specs assert on the reel's own data
 *  (caption bar, TitleSpotlight closer), not on EventSpotlight's fetched
 *  event body, so an unresolved/"missing" spotlight card is fine. */
function stubReelsFetch(opts: { reels?: ReelMeta[]; reelsById?: Record<string, Reel> }) {
  const fetchMock = vi.fn((url: string) => {
    const u = String(url);
    if (u === "/api/reels") {
      return Promise.resolve({ ok: true, json: () => Promise.resolve(opts.reels ?? []) });
    }
    const m = u.match(/^\/api\/reels\/([^/?]+)$/);
    if (m && m[1]) {
      const reel = opts.reelsById?.[decodeURIComponent(m[1])];
      return reel
        ? Promise.resolve({ ok: true, json: () => Promise.resolve(reel) })
        : Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve(null) });
    }
    return Promise.resolve({ ok: false, status: 404, json: () => Promise.resolve({}) });
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

const TWO_STOP_REEL: Reel = {
  id: "r1",
  title: "Two Stop Reel",
  created: "2026-08-01T10:00:00Z",
  author: "max",
  closer: "Thanks for watching",
  stops: [
    { sessionId: "s1", eventId: "e1", line: "First, the bug was found." },
    { sessionId: "s1", eventId: "e2", line: "Then, the fix landed." },
  ],
};

const THREE_STOP_REEL: Reel = {
  id: "r3",
  title: "Three Stop Reel",
  created: "2026-08-01T12:00:00Z",
  author: "max",
  closer: "Done",
  stops: [
    { sessionId: "s1", eventId: "e1", line: "Stop one." },
    { sessionId: "s1", eventId: "e2", line: "Stop two." },
    { sessionId: "s1", eventId: "e3", line: "Stop three." },
  ],
};

const ONE_STOP_REEL: Reel = {
  id: "r2",
  title: "One Stop Reel",
  created: "2026-08-01T11:00:00Z",
  author: "max",
  closer: "That's a wrap",
  stops: [{ sessionId: "s1", eventId: "e1", line: "Just one stop." }],
};

function playerRoute(reelId: string): HashRoute {
  return { view: "reels", reelId, reelAutoplay: true };
}

/** The bottom-bar counter is unified-slide-aware (reel-slide-standard):
 *  the total includes the opener/closer folded into `slides[]`, and the
 *  index/total pair carries a ` · {kind}` suffix for every non-title slide
 *  (see `captionFor` / the counter markup in ReelsView.tsx). All reels
 *  used below attach a closer, so a reel with N body stops counts N + 1
 *  slides total once playback reaches it. */
function counterText(n: number, total: number, kind = "spotlight"): string {
  return `${n} / ${total} · ${kind}`;
}

describe("when the reels list has no reels", () => {
  it("should render the empty-state copy", async () => {
    stubReelsFetch({ reels: [] });
    render(<ReelsView route={{ view: "reels" }} onNavigate={vi.fn()} />);

    await waitFor(() => expect(screen.getByTestId("reels-empty")).toBeInTheDocument());
    expect(
      screen.getByText("No reels yet. An agent can save one with the save_reel MCP tool."),
    ).toBeInTheDocument();
  });
});

describe("when the reels list has reels", () => {
  it("should render title/stopCount per row and navigate on row click", async () => {
    const onNavigate = vi.fn();
    stubReelsFetch({
      reels: [
        { id: "r1", title: "Two Stop Reel", created: "2026-08-01T10:00:00Z", author: "max", stopCount: 2 },
      ],
    });
    render(<ReelsView route={{ view: "reels" }} onNavigate={onNavigate} />);

    await waitFor(() => expect(screen.getByTestId("reel-row-r1")).toBeInTheDocument());
    expect(screen.getByText("Two Stop Reel")).toBeInTheDocument();
    expect(screen.getByText("2 stops")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("reel-row-r1"));
    expect(onNavigate).toHaveBeenCalledWith({ view: "reels", reelId: "r1" });
  });

  it("should navigate with reelAutoplay when the Play affordance is clicked", async () => {
    const onNavigate = vi.fn();
    stubReelsFetch({
      reels: [
        { id: "r1", title: "Two Stop Reel", created: "2026-08-01T10:00:00Z", author: "max", stopCount: 2 },
      ],
    });
    render(<ReelsView route={{ view: "reels" }} onNavigate={onNavigate} />);

    await waitFor(() => expect(screen.getByTestId("reel-play-r1")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("reel-play-r1"));
    expect(onNavigate).toHaveBeenCalledWith({ view: "reels", reelId: "r1", reelAutoplay: true });
  });
});

describe("when a reel plays a stop", () => {
  it("should render the stop's caption line and index/total", async () => {
    stubSpeechSynthesis();
    stubReelsFetch({ reelsById: { r1: TWO_STOP_REEL } });
    render(<ReelsView route={playerRoute("r1")} onNavigate={vi.fn()} />);

    await waitFor(() => expect(screen.getByTestId("reels-caption-bar")).toBeInTheDocument());
    expect(screen.getByText("First, the bug was found.")).toBeInTheDocument();
    // TWO_STOP_REEL has a closer, so the unified slide list is 2 stops + 1
    // closer = 3 slides total, not 2.
    expect(screen.getByText(counterText(1, 3))).toBeInTheDocument();
  });

  it("should ADVANCE to the next stop on a playback-surface click, not exit", async () => {
    stubSpeechSynthesis();
    const onNavigate = vi.fn();
    stubReelsFetch({ reelsById: { r1: TWO_STOP_REEL } });
    render(<ReelsView route={playerRoute("r1")} onNavigate={onNavigate} />);

    await waitFor(() => expect(screen.getByText(counterText(1, 3))).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("reels-playback-surface"));

    await waitFor(() => expect(screen.getByText(counterText(2, 3))).toBeInTheDocument());
    expect(screen.getByText("Then, the fix landed.")).toBeInTheDocument();
    // Finding-1 regression: a playback-surface click must never exit —
    // exit() calls onNavigate({view:"reels"}), which must not have fired.
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("should EXIT to the reels list route on Esc", async () => {
    stubSpeechSynthesis();
    const onNavigate = vi.fn();
    stubReelsFetch({ reelsById: { r1: TWO_STOP_REEL } });
    render(<ReelsView route={playerRoute("r1")} onNavigate={onNavigate} />);

    // Wait for the spotlight stage itself — it owns the Esc→exit keydown
    // listener via its own effect. The sibling caption bar (reels-caption-bar)
    // commits in the same render but is a different subtree, so waiting on it
    // does not guarantee the spotlight's listener is attached yet.
    await screen.findByTestId("event-spotlight");

    // Fire Escape inside the retry loop: under load the spotlight's keydown
    // effect can attach a tick after its element commits, so a single early
    // keyDown can land before there's a listener and be lost. Re-firing until
    // navigation happens removes that race — exit()→onNavigate is idempotent,
    // so extra fires never change the assertion.
    await waitFor(() => {
      fireEvent.keyDown(window, { key: "Escape", code: "Escape" });
      expect(onNavigate).toHaveBeenCalledWith({ view: "reels" });
    });
  });
});

describe("when the last stop's ADVANCE lands on a reel with a closer", () => {
  it("should render the closer's title via TitleSpotlight", async () => {
    stubSpeechSynthesis();
    stubReelsFetch({ reelsById: { r2: ONE_STOP_REEL } });
    render(<ReelsView route={playerRoute("r2")} onNavigate={vi.fn()} />);

    await waitFor(() => expect(screen.getByText("Just one stop.")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("reels-playback-surface"));

    await waitFor(() => expect(screen.getByTestId("title-spotlight")).toBeInTheDocument());
    expect(screen.getByText("That's a wrap")).toBeInTheDocument();
  });
});

describe("when a TTS engine fires onend on cancellation (Chrome quirk)", () => {
  it("should not phantom-double-advance a stop after a manual advance", async () => {
    stubSpeechSynthesisCancelFiresOnendLate();
    stubReelsFetch({ reelsById: { r3: THREE_STOP_REEL } });
    render(<ReelsView route={playerRoute("r3")} onNavigate={vi.fn()} />);

    // THREE_STOP_REEL has a closer, so unified slides = 3 stops + 1 closer
    // = 4 total.
    await waitFor(() => expect(screen.getByText(counterText(1, 4))).toBeInTheDocument());

    vi.useFakeTimers();
    try {
      // Manual advance: one click, 1 -> 2. This runs React's effect
      // cleanup for stop 1 (which calls cancel(), deferring stop 1's
      // utterance onend per the stub above) before mounting stop 2's
      // narration effect.
      fireEvent.click(screen.getByTestId("reels-playback-surface"));
      expect(screen.getByText(counterText(2, 4))).toBeInTheDocument();

      // Drain every pending timer, including the deferred onend for the
      // now-dead stop-1 closure. Buggy code has no way to know that
      // closure is dead: it schedules a fresh 2s ADVANCE timer and never
      // clears it, landing on stop 3 despite only one manual click.
      act(() => {
        vi.runAllTimers();
      });

      expect(screen.getByText(counterText(2, 4))).toBeInTheDocument();
      expect(screen.queryByText(counterText(3, 4))).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("when navigating a playing reel with the new controls", () => {
  it("should step back with the back button", async () => {
    stubSpeechSynthesis();
    stubReelsFetch({ reelsById: { r3: THREE_STOP_REEL } });
    render(<ReelsView route={playerRoute("r3")} onNavigate={vi.fn()} />);
    // THREE_STOP_REEL has a closer, so unified slides = 3 stops + 1 closer
    // = 4 total.
    await waitFor(() => expect(screen.getByText(counterText(1, 4))).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("reels-next"));
    expect(screen.getByText(counterText(2, 4))).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("reels-back"));
    expect(screen.getByText(counterText(1, 4))).toBeInTheDocument();
  });

  it("should jump straight to a stop via its progress segment", async () => {
    stubSpeechSynthesis();
    stubReelsFetch({ reelsById: { r3: THREE_STOP_REEL } });
    render(<ReelsView route={playerRoute("r3")} onNavigate={vi.fn()} />);
    await waitFor(() => expect(screen.getByText(counterText(1, 4))).toBeInTheDocument());
    // Segment 2 (0-indexed) is the third body stop in the unified list —
    // slide index 2 of 4, with the closer trailing at index 3.
    fireEvent.click(screen.getByTestId("reels-segment-2"));
    expect(screen.getByText(counterText(3, 4))).toBeInTheDocument();
    expect(screen.getByText("Stop three.")).toBeInTheDocument();
  });

  it("should honor ArrowRight / ArrowLeft", async () => {
    stubSpeechSynthesis();
    stubReelsFetch({ reelsById: { r3: THREE_STOP_REEL } });
    render(<ReelsView route={playerRoute("r3")} onNavigate={vi.fn()} />);
    await waitFor(() => expect(screen.getByText(counterText(1, 4))).toBeInTheDocument());
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(screen.getByText(counterText(2, 4))).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    expect(screen.getByText(counterText(1, 4))).toBeInTheDocument();
  });
});
