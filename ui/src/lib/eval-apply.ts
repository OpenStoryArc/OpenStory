/**
 * Eval-apply cycle extraction from session records.
 *
 * The recursive unit of agent work. Each cycle is:
 *   EVAL (model concludes) → APPLY* (zero or more tool dispatches)
 *
 * Terminal cycle: 0 tools (model chose text, no more work).
 * Non-terminal cycle: 1+ tools (model dispatched, expects results).
 *
 * This is the same structure at every depth:
 *   Main agent turns contain cycles.
 *   Subagent sessions ARE cycles.
 *   The coalgebra unfolds identically at every level.
 *
 * Pure function: records in, cycles out. No side effects.
 */

import type { WireRecord } from "@/types/wire-record";

export interface EvalApplyCycle {
  cycleNumber: number;
  evalText: string;
  tools: CycleTool[];
  isTerminal: boolean;
}

export interface CycleTool {
  name: string;
  summary: string;
}

/**
 * Extract eval-apply cycles from a sequence of WireRecords.
 *
 * The cycle boundary is deterministic:
 *   - assistant_message starts a new cycle (the eval)
 *   - tool_call accumulates on the current cycle (the apply)
 *   - next assistant_message finalizes the current cycle and starts a new one
 *
 * @param records — session records in chronological order
 * @returns cycles in order, last cycle is always terminal (if any exist)
 */
export function extractCycles(records: readonly WireRecord[]): EvalApplyCycle[] {
  const cycles: EvalApplyCycle[] = [];
  let currentEval: string | null = null;
  let currentTools: CycleTool[] = [];
  let cycleNum = 0;

  for (const r of records) {
    const rt = r.record_type;

    if (rt === "assistant_message") {
      // Finalize previous cycle if one was accumulating
      if (currentEval !== null) {
        cycleNum++;
        cycles.push({
          cycleNumber: cycleNum,
          evalText: currentEval,
          tools: currentTools,
          isTerminal: currentTools.length === 0,
        });
        currentTools = [];
      }

      // Start new cycle — extract eval text
      const payload = r.payload as Record<string, unknown> | undefined;
      let text = "";
      if (payload) {
        const content = payload.content;
        if (Array.isArray(content)) {
          for (const block of content) {
            if (
              typeof block === "object" &&
              block !== null &&
              (block as Record<string, unknown>).type === "text"
            ) {
              text = ((block as Record<string, unknown>).text as string) ?? "";
              break;
            }
          }
        } else if (typeof content === "string") {
          text = content;
        }
      }
      currentEval = text;
    } else if (rt === "tool_call") {
      // Accumulate tool on current cycle
      const payload = r.payload as Record<string, unknown> | undefined;
      const name = (payload?.name as string) ?? "?";
      const input = (payload?.input as Record<string, string>) ?? {};

      let summary = "";
      if (name === "Read" || name === "Write" || name === "Edit") {
        const fp = input.file_path ?? "";
        summary = fp.split("/").pop() ?? fp;
      } else if (name === "Grep" || name === "Glob") {
        summary = (input.pattern ?? "").slice(0, 30);
      } else if (name === "Bash") {
        summary = (input.command ?? "").slice(0, 50);
      } else if (name === "Agent") {
        summary = (input.description ?? "").slice(0, 50);
      } else {
        summary = Object.values(input).join(", ").slice(0, 30);
      }

      currentTools.push({ name, summary });
    }
    // tool_result, user_message, token_usage, etc. — skip
  }

  // Finalize last cycle
  if (currentEval !== null) {
    cycleNum++;
    cycles.push({
      cycleNumber: cycleNum,
      evalText: currentEval,
      tools: currentTools,
      isTerminal: currentTools.length === 0,
    });
  }

  return cycles;
}
