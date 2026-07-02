import { describe, it, expect } from "vitest";
import { findSentenceIndexByEvent } from "@/lib/story";
import type { PatternView } from "@/types/wire-record";

describe("findSentenceIndexByEvent — deep-link Story to an event", () => {
  const s = (id: string, events: string[]) =>
    ({ id, type: "turn.sentence", session_id: "x", events, metadata: {} }) as unknown as PatternView;

  it("finds the turn whose events include the id", () => {
    const list = [s("t1", ["e0", "e1"]), s("t2", ["e2", "e3"]), s("t3", ["e4"])];
    expect(findSentenceIndexByEvent(list, "e3")).toBe(1);
  });

  it("returns -1 for a missing id or no eventId", () => {
    const list = [s("t1", ["e0"])];
    expect(findSentenceIndexByEvent(list, "nope")).toBe(-1);
    expect(findSentenceIndexByEvent(list, undefined)).toBe(-1);
  });
});
