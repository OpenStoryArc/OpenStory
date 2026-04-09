//! Spec: Turn phase extraction — derive phase segments from records on the fly.
//!
//! Behavioral contract: given a session's WireRecord stream, extractTurnPhases
//! walks records sequentially, splits on user_message boundaries, and classifies
//! each turn by the set of tool_call records it contains. The classification
//! rules are ported from the legacy `TurnPhaseDetector` in
//! `rs/patterns/src/turn_phase.rs`:
//!
//!   - empty / no tools                              → "conversation"
//!   - only Read/Grep/Glob (+ Bash with explore-bias) → "exploration"
//!   - Edit or Write present, with bash > edits      → "implementation+testing"
//!   - Edit or Write present, otherwise              → "implementation"
//!   - Task tool present                             → "delegation"
//!   - Bash with > 5 calls                           → "testing"
//!   - Bash otherwise                                → "execution"
//!   - mixed shapes                                  → "mixed"
//!
//! This file is the spec for the post-cleanup behavior of
//! ui/src/lib/turn-phases.ts after the legacy `turn.phase` pattern type and
//! its detector are cut. The data flowing into the function is records, not
//! patterns — `turn.phase` no longer exists as a persisted pattern type.

import { describe, it, expect } from "vitest";
import { extractTurnPhases } from "@/lib/turn-phases";
import { scenario } from "../bdd";
import type { WireRecord } from "@/types/wire-record";

// ---------------------------------------------------------------------------
// Fixture builders — small, expressive helpers for crafting record streams.
// ---------------------------------------------------------------------------

let seq = 0;
function nextId(): string {
  return `rec-${++seq}`;
}

function userMessage(content: string = "hi"): WireRecord {
  return {
    id: nextId(),
    seq: seq,
    session_id: "spec-session",
    timestamp: "2026-04-08T00:00:00.000Z",
    record_type: "user_message",
    payload: { content, images: [] } as any,
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 0,
  };
}

function assistantText(content: string = "ok"): WireRecord {
  return {
    id: nextId(),
    seq: seq,
    session_id: "spec-session",
    timestamp: "2026-04-08T00:00:01.000Z",
    record_type: "assistant_message",
    payload: { content, images: [] } as any,
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 0,
  };
}

function toolCall(name: string): WireRecord {
  return {
    id: nextId(),
    seq: seq,
    session_id: "spec-session",
    timestamp: "2026-04-08T00:00:02.000Z",
    record_type: "tool_call",
    payload: { call_id: `c-${seq}`, name, input: {}, raw_input: {} } as any,
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 0,
  };
}

function repeat<T>(builder: () => T, n: number): T[] {
  return Array.from({ length: n }, () => builder());
}

// ---------------------------------------------------------------------------
// Specs
// ---------------------------------------------------------------------------

describe("extractTurnPhases — derived from records (no turn.phase patterns)", () => {
  it("returns empty for no records", () => {
    scenario(
      () => [] as WireRecord[],
      (records) => extractTurnPhases(records),
      (segments) => {
        expect(segments).toEqual([]);
      },
    );
  });

  describe("when a turn has no tool calls", () => {
    it("classifies the turn as 'conversation'", () => {
      scenario(
        () => [userMessage("just talking"), assistantText("noted")],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments).toHaveLength(1);
          expect(segments[0]!.phase).toBe("conversation");
          expect(segments[0]!.eventCount).toBe(2);
        },
      );
    });
  });

  describe("when a turn calls only Read/Grep/Glob", () => {
    it("classifies the turn as 'exploration'", () => {
      scenario(
        () => [
          userMessage("look around"),
          assistantText("checking"),
          toolCall("Read"),
          toolCall("Grep"),
          toolCall("Glob"),
        ],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments).toHaveLength(1);
          expect(segments[0]!.phase).toBe("exploration");
        },
      );
    });

    it("still classifies as 'exploration' when Bash is present but explore tools dominate", () => {
      scenario(
        () => [
          userMessage("look"),
          ...repeat(() => toolCall("Read"), 4),
          toolCall("Bash"),
        ],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments[0]!.phase).toBe("exploration");
        },
      );
    });
  });

  describe("when a turn calls Edit or Write", () => {
    it("classifies as 'implementation' when Bash is absent or minimal", () => {
      scenario(
        () => [userMessage("fix the bug"), toolCall("Edit")],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments[0]!.phase).toBe("implementation");
        },
      );
    });

    it("classifies as 'implementation+testing' when Bash calls outnumber edits", () => {
      scenario(
        () => [
          userMessage("fix and test"),
          toolCall("Edit"),
          ...repeat(() => toolCall("Bash"), 3),
        ],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments[0]!.phase).toBe("implementation+testing");
        },
      );
    });
  });

  describe("when a turn invokes the Task (subagent) tool", () => {
    it("classifies the turn as 'delegation'", () => {
      scenario(
        () => [userMessage("delegate"), toolCall("Task")],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments[0]!.phase).toBe("delegation");
        },
      );
    });
  });

  describe("when a turn runs lots of Bash and nothing else", () => {
    it("classifies as 'testing' when Bash count exceeds 5", () => {
      scenario(
        () => [userMessage("run tests"), ...repeat(() => toolCall("Bash"), 6)],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments[0]!.phase).toBe("testing");
        },
      );
    });

    it("classifies as 'execution' for a small Bash count", () => {
      scenario(
        () => [userMessage("run a thing"), toolCall("Bash")],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments[0]!.phase).toBe("execution");
        },
      );
    });
  });

  describe("when records contain multiple turn boundaries", () => {
    it("emits one segment per turn, in order", () => {
      scenario(
        () => [
          // Turn 1: conversation
          userMessage("hello"),
          assistantText("hi"),
          // Turn 2: exploration
          userMessage("look around"),
          toolCall("Read"),
          toolCall("Grep"),
          // Turn 3: implementation
          userMessage("now fix it"),
          toolCall("Edit"),
        ],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments.map((s) => s.phase)).toEqual([
            "conversation",
            "exploration",
            "implementation",
          ]);
        },
      );
    });

    it("populates eventCount and events per segment from the underlying records", () => {
      scenario(
        () => [
          userMessage("a"),
          assistantText("b"),
          toolCall("Read"),
          userMessage("c"),
          toolCall("Edit"),
        ],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments).toHaveLength(2);
          expect(segments[0]!.eventCount).toBe(3);
          expect(segments[1]!.eventCount).toBe(2);
          expect(segments[0]!.events).toHaveLength(3);
          expect(segments[1]!.events).toHaveLength(2);
        },
      );
    });
  });

  describe("when no rules match", () => {
    it("falls back to 'mixed'", () => {
      // A turn that uses an unrecognized tool only
      scenario(
        () => [userMessage("strange"), toolCall("WebFetch")],
        (records) => extractTurnPhases(records),
        (segments) => {
          expect(segments[0]!.phase).toBe("mixed");
        },
      );
    });
  });
});
