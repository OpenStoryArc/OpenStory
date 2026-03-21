/** Pure transforms for conversation view — group entries into turns. */

import type {
  ConversationEntry,
  ToolRoundtripEntry,
} from "@/types/view-record";

/** A logical turn: user prompt → thinking → tool calls → response. */
export interface ConversationTurn {
  readonly prompt: string | null;
  readonly promptTimestamp: string | null;
  readonly thinking: string | null;
  readonly toolCalls: readonly ToolRoundtripEntry[];
  readonly response: string | null;
  readonly responseTimestamp: string | null;
}

/** Group flat conversation entries into logical turns.
 *  A turn starts at each user_message and collects everything until the next user_message. */
export function groupIntoTurns(entries: readonly ConversationEntry[]): ConversationTurn[] {
  if (entries.length === 0) return [];

  const turns: ConversationTurn[] = [];
  let current: {
    prompt: string | null;
    promptTimestamp: string | null;
    thinking: string | null;
    toolCalls: ToolRoundtripEntry[];
    response: string | null;
    responseTimestamp: string | null;
  } = { prompt: null, promptTimestamp: null, thinking: null, toolCalls: [], response: null, responseTimestamp: null };
  let hasTurn = false;

  for (const entry of entries) {
    switch (entry.entry_type) {
      case "user_message":
        // Start a new turn (flush previous if exists)
        if (hasTurn) turns.push({ ...current, toolCalls: [...current.toolCalls] });
        current = {
          prompt: entry.payload.text ?? null,
          promptTimestamp: entry.timestamp,
          thinking: null,
          toolCalls: [],
          response: null,
          responseTimestamp: null,
        };
        hasTurn = true;
        break;

      case "reasoning":
        // Append thinking (could be multiple thinking blocks)
        if (!hasTurn) {
          current = { prompt: null, promptTimestamp: null, thinking: null, toolCalls: [], response: null, responseTimestamp: null };
          hasTurn = true;
        }
        current.thinking = current.thinking
          ? current.thinking + "\n" + entry.payload.text
          : entry.payload.text;
        break;

      case "tool_roundtrip":
        if (!hasTurn) {
          current = { prompt: null, promptTimestamp: null, thinking: null, toolCalls: [], response: null, responseTimestamp: null };
          hasTurn = true;
        }
        current.toolCalls.push(entry);
        break;

      case "assistant_message":
        if (!hasTurn) {
          current = { prompt: null, promptTimestamp: null, thinking: null, toolCalls: [], response: null, responseTimestamp: null };
          hasTurn = true;
        }
        // Last assistant message wins as the response
        current.response = entry.payload.text ?? null;
        current.responseTimestamp = entry.timestamp;
        break;

      // token_usage and system entries are skipped
      default:
        break;
    }
  }

  if (hasTurn) turns.push({ ...current, toolCalls: [...current.toolCalls] });
  return turns;
}

/** Count total tool calls across all turns. */
export function totalToolCalls(turns: readonly ConversationTurn[]): number {
  return turns.reduce((sum, t) => sum + t.toolCalls.length, 0);
}
