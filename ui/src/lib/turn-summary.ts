/** Per-turn summary stats, computed from records between consecutive turn_end events. */

import type { WireRecord } from "@/types/wire-record";
import type { ToolCall, ToolResult, TurnEnd } from "@/types/view-record";

export interface TurnSummary {
  readonly toolCalls: number;
  readonly errors: number;
  readonly edits: number;
  readonly durationMs: number | undefined;
}

const EDIT_TOOLS = new Set(["Edit", "Write"]);

/** Walk records and compute per-turn summaries.
 *  Returns a map from turn_end event ID → TurnSummary.
 *  Records between the start (or previous turn_end) and each turn_end
 *  are counted for tool calls, errors, and file edits. */
export function computeTurnSummaries(
  records: readonly WireRecord[],
): Map<string, TurnSummary> {
  const result = new Map<string, TurnSummary>();

  let toolCalls = 0;
  let errors = 0;
  let edits = 0;

  for (const r of records) {
    switch (r.record_type) {
      case "tool_call": {
        toolCalls++;
        const tc = r.payload as ToolCall;
        if (EDIT_TOOLS.has(tc.name)) edits++;
        break;
      }
      case "tool_result": {
        const tr = r.payload as ToolResult;
        if (tr.is_error) errors++;
        break;
      }
      case "turn_end": {
        const te = r.payload as TurnEnd;
        result.set(r.id, {
          toolCalls,
          errors,
          edits,
          durationMs: te.duration_ms ?? undefined,
        });
        // Reset counters for next turn
        toolCalls = 0;
        errors = 0;
        edits = 0;
        break;
      }
    }
  }

  return result;
}
