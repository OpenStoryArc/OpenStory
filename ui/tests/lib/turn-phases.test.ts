//! Spec: Turn phase extraction — pull turn.phase patterns into renderable segments.
//!
//! Pure function: PatternView[] → TurnPhaseSegment[]

import { describe, it, expect } from "vitest";
import { extractTurnPhases } from "@/lib/turn-phases";
import type { PatternView } from "@/types/wire-record";

describe("extractTurnPhases", () => {
  it("should return empty for no patterns", () => {
    expect(extractTurnPhases([])).toEqual([]);
  });

  it("should extract only turn.phase patterns", () => {
    const patterns: PatternView[] = [
      { type: "turn.phase", label: "conversation (3 events)", session_id: "", events: ["e1", "e2", "e3"] },
      { type: "git.workflow", label: "Git: status → commit", session_id: "", events: ["e4", "e5"] },
      { type: "turn.phase", label: "implementation (5 events)", session_id: "", events: ["e4", "e5", "e6", "e7", "e8"] },
    ];
    const segments = extractTurnPhases(patterns);
    expect(segments).toHaveLength(2);
    expect(segments[0]!.phase).toBe("conversation");
    expect(segments[0]!.eventCount).toBe(3);
    expect(segments[1]!.phase).toBe("implementation");
    expect(segments[1]!.eventCount).toBe(5);
  });

  it("should parse phase name from label", () => {
    const patterns: PatternView[] = [
      { type: "turn.phase", label: "exploration (10 events)", session_id: "", events: Array(10).fill("e") },
      { type: "turn.phase", label: "implementation+testing (7 events)", session_id: "", events: Array(7).fill("e") },
      { type: "turn.phase", label: "delegation (2 events)", session_id: "", events: Array(2).fill("e") },
    ];
    const segments = extractTurnPhases(patterns);
    expect(segments.map(s => s.phase)).toEqual(["exploration", "implementation+testing", "delegation"]);
  });

  it("should handle label without parenthetical", () => {
    const patterns: PatternView[] = [
      { type: "turn.phase", label: "conversation", session_id: "", events: ["e1"] },
    ];
    const segments = extractTurnPhases(patterns);
    expect(segments[0]!.phase).toBe("conversation");
    expect(segments[0]!.eventCount).toBe(1);
  });
});
