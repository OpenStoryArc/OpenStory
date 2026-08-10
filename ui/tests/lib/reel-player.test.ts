import { describe, it, expect } from "vitest";
import {
  initialReelPlayerState,
  isReelPaused,
  isReelPlaying,
  reelPlayerReduce,
  type ReelPlayerState,
} from "@/lib/reel-player";

const ctx = { stopCount: 3, hasCloser: true };

describe("when a reel plays", () => {
  it("should start at stop 0 unpaused on PLAY", () => {
    expect(reelPlayerReduce(initialReelPlayerState, { type: "PLAY" }, ctx)).toEqual({
      phase: "stop",
      index: 0,
      paused: false,
    });
  });
  it("should walk stops in order on ADVANCE", () => {
    let s: ReelPlayerState = { phase: "stop", index: 0, paused: false };
    s = reelPlayerReduce(s, { type: "ADVANCE" }, ctx);
    expect(s).toEqual({ phase: "stop", index: 1, paused: false });
  });
  it("should show the closer after the last stop", () => {
    const s = reelPlayerReduce(
      { phase: "stop", index: 2, paused: false },
      { type: "ADVANCE" },
      ctx,
    );
    expect(s).toEqual({ phase: "closer", paused: false });
  });
  it("should be done after the closer", () => {
    expect(
      reelPlayerReduce({ phase: "closer", paused: false }, { type: "ADVANCE" }, ctx),
    ).toEqual({
      phase: "done",
    });
  });
  it("should skip the closer when the reel has none", () => {
    const s = reelPlayerReduce(
      { phase: "stop", index: 2, paused: false },
      { type: "ADVANCE" },
      { stopCount: 3, hasCloser: false },
    );
    expect(s).toEqual({ phase: "done" });
  });
  it("should EXIT to idle from any phase", () => {
    for (const from of [
      { phase: "stop", index: 1, paused: false },
      { phase: "closer", paused: true },
      { phase: "done" },
    ] as ReelPlayerState[]) {
      expect(reelPlayerReduce(from, { type: "EXIT" }, ctx)).toEqual({ phase: "idle" });
    }
  });
  it("should open with the opener card when the reel has one (BLUF first)", () => {
    const withOpener = { stopCount: 3, hasCloser: true, hasOpener: true };
    expect(reelPlayerReduce(initialReelPlayerState, { type: "PLAY" }, withOpener)).toEqual({
      phase: "opener",
      paused: false,
    });
    expect(
      reelPlayerReduce({ phase: "opener", paused: false }, { type: "ADVANCE" }, withOpener),
    ).toEqual({
      phase: "stop",
      index: 0,
      paused: false,
    });
    expect(
      reelPlayerReduce({ phase: "opener", paused: false }, { type: "EXIT" }, withOpener),
    ).toEqual({
      phase: "idle",
    });
  });

  it("should skip straight to stop 0 when there is no opener", () => {
    expect(reelPlayerReduce(initialReelPlayerState, { type: "PLAY" }, ctx)).toEqual({
      phase: "stop",
      index: 0,
      paused: false,
    });
  });

  it("should step BACK through stops, into the opener, and from the closer", () => {
    const withOpener = { stopCount: 3, hasCloser: true, hasOpener: true };
    expect(
      reelPlayerReduce({ phase: "stop", index: 2, paused: false }, { type: "BACK" }, withOpener),
    ).toEqual({
      phase: "stop",
      index: 1,
      paused: false,
    });
    expect(
      reelPlayerReduce({ phase: "stop", index: 0, paused: false }, { type: "BACK" }, withOpener),
    ).toEqual({
      phase: "opener",
      paused: false,
    });
    expect(
      reelPlayerReduce({ phase: "stop", index: 0, paused: false }, { type: "BACK" }, ctx),
    ).toEqual({
      phase: "stop",
      index: 0,
      paused: false,
    });
    expect(
      reelPlayerReduce({ phase: "closer", paused: false }, { type: "BACK" }, ctx),
    ).toEqual({
      phase: "stop",
      index: 2,
      paused: false,
    });
  });

  it("should JUMP to a valid stop and ignore out-of-range targets", () => {
    expect(
      reelPlayerReduce(
        { phase: "stop", index: 0, paused: false },
        { type: "JUMP", index: 2 },
        ctx,
      ),
    ).toEqual({
      phase: "stop",
      index: 2,
      paused: false,
    });
    expect(
      reelPlayerReduce({ phase: "closer", paused: true }, { type: "JUMP", index: 1 }, ctx),
    ).toEqual({
      phase: "stop",
      index: 1,
      paused: true,
    });
    expect(
      reelPlayerReduce(
        { phase: "stop", index: 1, paused: false },
        { type: "JUMP", index: 9 },
        ctx,
      ),
    ).toEqual({
      phase: "stop",
      index: 1,
      paused: false,
    });
  });

  it("should ignore ADVANCE while idle and PLAY restarts from done", () => {
    expect(reelPlayerReduce({ phase: "idle" }, { type: "ADVANCE" }, ctx)).toEqual({
      phase: "idle",
    });
    expect(reelPlayerReduce({ phase: "done" }, { type: "PLAY" }, ctx)).toEqual({
      phase: "stop",
      index: 0,
      paused: false,
    });
  });
});

describe("play / pause", () => {
  it("should pause and resume without changing stop index", () => {
    const playing: ReelPlayerState = { phase: "stop", index: 1, paused: false };
    const paused = reelPlayerReduce(playing, { type: "PAUSE" }, ctx);
    expect(paused).toEqual({ phase: "stop", index: 1, paused: true });
    expect(isReelPaused(paused)).toBe(true);
    expect(isReelPlaying(paused)).toBe(false);
    const resumed = reelPlayerReduce(paused, { type: "RESUME" }, ctx);
    expect(resumed).toEqual({ phase: "stop", index: 1, paused: false });
    expect(isReelPlaying(resumed)).toBe(true);
  });

  it("should toggle pause", () => {
    const s = reelPlayerReduce(
      { phase: "stop", index: 0, paused: false },
      { type: "TOGGLE_PAUSE" },
      ctx,
    );
    expect(s).toEqual({ phase: "stop", index: 0, paused: true });
    expect(
      reelPlayerReduce(s, { type: "TOGGLE_PAUSE" }, ctx),
    ).toEqual({ phase: "stop", index: 0, paused: false });
  });

  it("should keep paused when clicking through slides (ADVANCE/BACK)", () => {
    let s: ReelPlayerState = { phase: "stop", index: 0, paused: true };
    s = reelPlayerReduce(s, { type: "ADVANCE" }, ctx);
    expect(s).toEqual({ phase: "stop", index: 1, paused: true });
    s = reelPlayerReduce(s, { type: "BACK" }, ctx);
    expect(s).toEqual({ phase: "stop", index: 0, paused: true });
  });
});
