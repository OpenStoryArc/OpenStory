//! Spec: Pattern index — maps event IDs to their pattern memberships.
//!
//! Pure function: patterns[] → Map<eventId, PatternView[]>

import { describe, it, expect } from "vitest";
import { buildPatternIndex } from "@/lib/pattern-index";
import type { PatternView } from "@/types/wire-record";

describe("buildPatternIndex", () => {
  it("should return empty map for empty patterns", () => {
    const index = buildPatternIndex([]);
    expect(index.size).toBe(0);
  });

  it("should map each event_id to its pattern", () => {
    const patterns: PatternView[] = [
      { type: "turn.sentence", label: "Claude committed changes", session_id: "", events: ["e1", "e2", "e3"] },
    ];
    const index = buildPatternIndex(patterns);

    expect(index.get("e1")).toEqual([patterns[0]]);
    expect(index.get("e2")).toEqual([patterns[0]]);
    expect(index.get("e3")).toEqual([patterns[0]]);
    expect(index.has("e4")).toBe(false);
  });

  it("should handle multiple patterns sharing an event", () => {
    const p1: PatternView = { type: "eval_apply.eval", label: "eval", session_id: "", events: ["e1", "e2", "e3"] };
    const p2: PatternView = { type: "eval_apply.apply", label: "apply", session_id: "", events: ["e2", "e3", "e4"] };
    const index = buildPatternIndex([p1, p2]);

    expect(index.get("e1")).toEqual([p1]);
    expect(index.get("e2")).toEqual([p1, p2]);
    expect(index.get("e3")).toEqual([p1, p2]);
    expect(index.get("e4")).toEqual([p2]);
  });

  it("should handle patterns with no events", () => {
    const patterns: PatternView[] = [
      { type: "turn.sentence", label: "empty sentence", session_id: "", events: [] },
    ];
    const index = buildPatternIndex(patterns);
    expect(index.size).toBe(0);
  });
});
