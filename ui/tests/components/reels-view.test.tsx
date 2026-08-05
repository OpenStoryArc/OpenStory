/** ReelsView — list + spotlight player.
 *
 *  Covers the click-to-advance contract fixed after code review: a click on
 *  the reel playback surface must ADVANCE, not fall through to
 *  EventSpotlight/TitleSpotlight's own backdrop `onClick={onClose}` (which
 *  would exit the reel instead). Esc still exits. See the
 *  `PlaybackClickSurface` doc comment in ReelsView.tsx for the trap this
 *  guards against. */

import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { ReelsView } from "@/components/reels/ReelsView";
import type { HashRoute } from "@/lib/hash-route";
import type { Reel, ReelMeta } from "@/lib/reels-api";

afterEach(() => vi.unstubAllGlobals());

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
    expect(screen.getByText("1 / 2")).toBeInTheDocument();
  });

  it("should ADVANCE to the next stop on a playback-surface click, not exit", async () => {
    stubSpeechSynthesis();
    const onNavigate = vi.fn();
    stubReelsFetch({ reelsById: { r1: TWO_STOP_REEL } });
    render(<ReelsView route={playerRoute("r1")} onNavigate={onNavigate} />);

    await waitFor(() => expect(screen.getByText("1 / 2")).toBeInTheDocument());
    fireEvent.click(screen.getByTestId("reels-playback-surface"));

    await waitFor(() => expect(screen.getByText("2 / 2")).toBeInTheDocument());
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

    await waitFor(() => expect(screen.getByTestId("reels-caption-bar")).toBeInTheDocument());
    fireEvent.keyDown(window, { key: "Escape", code: "Escape" });

    await waitFor(() => expect(onNavigate).toHaveBeenCalledWith({ view: "reels" }));
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
