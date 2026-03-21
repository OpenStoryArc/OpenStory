import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { computeTurnSummaries, type TurnSummary } from "@/lib/turn-summary";
import type { WireRecord } from "@/types/wire-record";

// Helper: create a minimal WireRecord for testing
function rec(
  id: string,
  record_type: string,
  payload: Record<string, unknown> = {},
): WireRecord {
  return {
    id,
    seq: 0,
    session_id: "s1",
    timestamp: `2025-01-12T10:00:0${id}Z`,
    record_type: record_type as WireRecord["record_type"],
    payload: payload as WireRecord["payload"],
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 0,
  };
}

function toolCall(id: string, name: string): WireRecord {
  return rec(id, "tool_call", { name, call_id: "", input: {}, raw_input: {}, typed_input: null });
}

function toolResult(id: string, isError = false): WireRecord {
  return rec(id, "tool_result", { call_id: "", output: "", is_error: isError });
}

function turnEnd(id: string, durationMs?: number): WireRecord {
  return rec(id, "turn_end", { duration_ms: durationMs, turn_id: null, reason: "end_turn" });
}

// ── Boundary table ──────────────────────────────────────────────────

const BOUNDARY_TABLE: [
  string,            // description
  WireRecord[],      // records
  Map<string, TurnSummary>, // expected summaries (keyed by turn_end id)
][] = [
  // No turns at all
  ["no turns", [toolCall("1", "Bash"), toolResult("2")], new Map()],

  // Single turn
  [
    "single turn",
    [
      rec("1", "user_message", { content: "hi" }),
      toolCall("2", "Bash"),
      toolResult("3"),
      toolCall("4", "Edit"),
      toolResult("5"),
      turnEnd("6", 5000),
    ],
    new Map([
      ["6", { toolCalls: 2, errors: 0, edits: 1, durationMs: 5000 }],
    ]),
  ],

  // Multi-turn
  [
    "multi-turn",
    [
      toolCall("1", "Read"),
      turnEnd("2", 1000),
      toolCall("3", "Bash"),
      toolCall("4", "Bash"),
      toolResult("5", true),
      turnEnd("6", 2000),
    ],
    new Map([
      ["2", { toolCalls: 1, errors: 0, edits: 0, durationMs: 1000 }],
      ["6", { toolCalls: 2, errors: 1, edits: 0, durationMs: 2000 }],
    ]),
  ],

  // Turn with errors
  [
    "turn with errors",
    [
      toolCall("1", "Bash"),
      toolResult("2", true),
      toolCall("3", "Bash"),
      toolResult("4", true),
      turnEnd("5", 3000),
    ],
    new Map([
      ["5", { toolCalls: 2, errors: 2, edits: 0, durationMs: 3000 }],
    ]),
  ],

  // Turn with file edits (Edit + Write)
  [
    "turn with file edits",
    [
      toolCall("1", "Edit"),
      toolCall("2", "Write"),
      toolCall("3", "Edit"),
      turnEnd("4", 1500),
    ],
    new Map([
      ["4", { toolCalls: 3, errors: 0, edits: 3, durationMs: 1500 }],
    ]),
  ],
];

describe("computeTurnSummaries — boundary table", () => {
  it.each(BOUNDARY_TABLE)(
    "%s",
    (_desc, records, expected) => {
      scenario(
        () => records,
        (recs) => computeTurnSummaries(recs),
        (result) => {
          expect(result.size).toBe(expected.size);
          for (const [id, exp] of expected) {
            const actual = result.get(id);
            expect(actual, `summary for ${id}`).toBeDefined();
            expect(actual).toEqual(exp);
          }
        },
      );
    },
  );
});
