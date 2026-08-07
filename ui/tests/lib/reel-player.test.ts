import { describe, it, expect } from "vitest";
import {
  initialReelPlayerState,
  reelPlayerReduce,
  type ReelPlayerState,
} from "@/lib/reel-player";

const ctx = { stopCount: 3, hasCloser: true };

describe("when a reel plays", () => {
  it("should start at stop 0 on PLAY", () => {
    expect(reelPlayerReduce(initialReelPlayerState, { type: "PLAY" }, ctx)).toEqual({
      phase: "stop",
      index: 0,
    });
  });
  it("should walk stops in order on ADVANCE", () => {
    let s: ReelPlayerState = { phase: "stop", index: 0 };
    s = reelPlayerReduce(s, { type: "ADVANCE" }, ctx);
    expect(s).toEqual({ phase: "stop", index: 1 });
  });
  it("should show the closer after the last stop", () => {
    const s = reelPlayerReduce({ phase: "stop", index: 2 }, { type: "ADVANCE" }, ctx);
    expect(s).toEqual({ phase: "closer" });
  });
  it("should be done after the closer", () => {
    expect(reelPlayerReduce({ phase: "closer" }, { type: "ADVANCE" }, ctx)).toEqual({
      phase: "done",
    });
  });
  it("should skip the closer when the reel has none", () => {
    const s = reelPlayerReduce(
      { phase: "stop", index: 2 },
      { type: "ADVANCE" },
      { stopCount: 3, hasCloser: false },
    );
    expect(s).toEqual({ phase: "done" });
  });
  it("should EXIT to idle from any phase", () => {
    for (const from of [
      { phase: "stop", index: 1 },
      { phase: "closer" },
      { phase: "done" },
    ] as ReelPlayerState[]) {
      expect(reelPlayerReduce(from, { type: "EXIT" }, ctx)).toEqual({ phase: "idle" });
    }
  });
  it("should open with the opener card when the reel has one (BLUF first)", () => {
    const withOpener = { stopCount: 3, hasCloser: true, hasOpener: true };
    expect(reelPlayerReduce(initialReelPlayerState, { type: "PLAY" }, withOpener)).toEqual({
      phase: "opener",
    });
    expect(reelPlayerReduce({ phase: "opener" }, { type: "ADVANCE" }, withOpener)).toEqual({
      phase: "stop",
      index: 0,
    });
    expect(reelPlayerReduce({ phase: "opener" }, { type: "EXIT" }, withOpener)).toEqual({
      phase: "idle",
    });
  });

  it("should skip straight to stop 0 when there is no opener", () => {
    expect(reelPlayerReduce(initialReelPlayerState, { type: "PLAY" }, ctx)).toEqual({
      phase: "stop",
      index: 0,
    });
  });

  it("should step BACK through stops, into the opener, and from the closer", () => {
    const withOpener = { stopCount: 3, hasCloser: true, hasOpener: true };
    expect(reelPlayerReduce({ phase: "stop", index: 2 }, { type: "BACK" }, withOpener)).toEqual({
      phase: "stop",
      index: 1,
    });
    expect(reelPlayerReduce({ phase: "stop", index: 0 }, { type: "BACK" }, withOpener)).toEqual({
      phase: "opener",
    });
    expect(reelPlayerReduce({ phase: "stop", index: 0 }, { type: "BACK" }, ctx)).toEqual({
      phase: "stop",
      index: 0,
    });
    expect(reelPlayerReduce({ phase: "closer" }, { type: "BACK" }, ctx)).toEqual({
      phase: "stop",
      index: 2,
    });
  });

  it("should JUMP to a valid stop and ignore out-of-range targets", () => {
    expect(reelPlayerReduce({ phase: "stop", index: 0 }, { type: "JUMP", index: 2 }, ctx)).toEqual({
      phase: "stop",
      index: 2,
    });
    expect(reelPlayerReduce({ phase: "closer" }, { type: "JUMP", index: 1 }, ctx)).toEqual({
      phase: "stop",
      index: 1,
    });
    expect(reelPlayerReduce({ phase: "stop", index: 1 }, { type: "JUMP", index: 9 }, ctx)).toEqual({
      phase: "stop",
      index: 1,
    });
    expect(reelPlayerReduce({ phase: "stop", index: 1 }, { type: "JUMP", index: -1 }, ctx)).toEqual({
      phase: "stop",
      index: 1,
    });
  });

  it("should ignore ADVANCE while idle and PLAY restarts from done", () => {
    expect(reelPlayerReduce({ phase: "idle" }, { type: "ADVANCE" }, ctx)).toEqual({
      phase: "idle",
    });
    expect(reelPlayerReduce({ phase: "done" }, { type: "PLAY" }, ctx)).toEqual({
      phase: "stop",
      index: 0,
    });
  });
});
