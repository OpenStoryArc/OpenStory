/** The Live-tab facets (Conversation / Code / Commands / Tests / Git / Errors /
 *  Thinking / Plans / Agents), applied to the PAIRED conversation entries the
 *  Explore session view renders. Mirrors ui/src/lib/timeline-filters.ts (which
 *  predicates over WireRecords) for the ConversationEntry shape, so Explore's
 *  conversation gets the same filter experience as Live. Pure + testable. */

import type { ConversationEntry, ToolCall, ToolResult } from "@/types/view-record";

export const CONVERSATION_FACETS = [
  "all", "conversation", "code", "commands", "tests", "git", "errors", "thinking", "plans", "agents",
] as const;
export type ConversationFacet = (typeof CONVERSATION_FACETS)[number];

function callName(e: ConversationEntry): string {
  return e.entry_type === "tool_roundtrip" ? ((e.call.payload as ToolCall)?.name ?? "") : "";
}
function bashCommand(e: ConversationEntry): string {
  if (e.entry_type !== "tool_roundtrip") return "";
  const c = e.call.payload as ToolCall;
  if (c?.name !== "Bash") return "";
  const raw = c.raw_input as Record<string, unknown> | undefined;
  return (raw?.command as string) ?? "";
}
function output(e: ConversationEntry): string {
  if (e.entry_type !== "tool_roundtrip" || !e.result) return "";
  return (e.result.payload as ToolResult)?.output ?? "";
}
function isErrorResult(e: ConversationEntry): boolean {
  return e.entry_type === "tool_roundtrip" && !!e.result && Boolean((e.result.payload as ToolResult)?.is_error);
}

/** True if `entry` belongs to `facet`. */
export function conversationEntryMatches(entry: ConversationEntry, facet: ConversationFacet): boolean {
  switch (facet) {
    case "all":
      return true;
    case "conversation":
      return entry.entry_type === "user_message" || entry.entry_type === "assistant_message";
    case "code":
      return ["Read", "Edit", "Write", "Glob", "Grep"].includes(callName(entry));
    case "commands":
      return callName(entry) === "Bash";
    case "tests": {
      const cmd = bashCommand(entry);
      if (/cargo test|npm test|npx vitest|npx jest|pytest/.test(cmd)) return true;
      const out = output(entry);
      return out.includes("test result:") || /\d+\s+(?:passed|failed)/.test(out) || out.includes("FAILED");
    }
    case "git": {
      if (bashCommand(entry).includes("git ")) return true;
      const out = output(entry);
      return out.includes("commit ") || out.includes("branch ");
    }
    case "errors": {
      if (isErrorResult(entry)) return true;
      const out = output(entry);
      return out.includes("error[") || /TS\d/.test(out) || out.includes("SyntaxError");
    }
    case "thinking":
      return entry.entry_type === "reasoning";
    case "plans":
      return callName(entry) === "EnterPlanMode" || callName(entry) === "ExitPlanMode";
    case "agents":
      return callName(entry) === "Agent";
  }
}

/** Per-facet counts for the pills; `all` = total. */
export function facetCounts(entries: readonly ConversationEntry[]): Record<ConversationFacet, number> {
  const counts = Object.fromEntries(CONVERSATION_FACETS.map((f) => [f, 0])) as Record<ConversationFacet, number>;
  for (const e of entries) {
    for (const f of CONVERSATION_FACETS) {
      if (f !== "all" && conversationEntryMatches(e, f)) counts[f]++;
    }
  }
  counts.all = entries.length;
  return counts;
}
