/** Pure transforms for ViewRecord display.
 *  Replaces the 40+ if/else branches in event-transforms.ts with
 *  exhaustive matching on the typed RecordType and RecordPayload. */

import type {
  ViewRecord,
  RecordType,
  ToolCall,
  ToolResult,
  AssistantMessage,
  ContentBlock,
  TokenUsage,
  TurnEnd,
  ErrorRecord,
  UserMessage,
} from "@/types/view-record";
import { toolInputSummary } from "@/types/view-record";
import { isGitCommand, gitCommandRisk, GIT_RISK_COLORS } from "@/lib/git-commands";

// ---------------------------------------------------------------------------
// viewRecordLabel — human-readable label for a record type
// ---------------------------------------------------------------------------

const LABELS: Record<RecordType, string> = {
  user_message: "Prompt",
  assistant_message: "Response",
  reasoning: "Thinking",
  tool_call: "Tool Use",
  tool_result: "Result",
  turn_end: "Complete",
  token_usage: "Tokens",
  system_event: "System",
  error: "Error",
  session_meta: "Start",
  turn_start: "Turn",
  context_compaction: "Compact",
  file_snapshot: "Snapshot",
};

export function viewRecordLabel(recordType: RecordType): string {
  return LABELS[recordType] ?? recordType;
}

// ---------------------------------------------------------------------------
// viewRecordSummary — one-line summary of a ViewRecord
// ---------------------------------------------------------------------------

export function viewRecordSummary(record: ViewRecord): string {
  switch (record.record_type) {
    case "user_message": {
      const p = record.payload as UserMessage;
      return typeof p.content === "string" ? p.content : extractText(p.content);
    }
    case "assistant_message": {
      const p = record.payload as AssistantMessage;
      return extractText(p.content);
    }
    case "reasoning":
      return "Thinking...";
    case "tool_call": {
      const p = record.payload as ToolCall;
      const detail = toolInputSummary(p.typed_input);
      return detail ? `${p.name}: ${detail}` : p.name;
    }
    case "tool_result": {
      const p = record.payload as ToolResult;
      const text = p.output ?? "";
      return text.length > 120 ? text.slice(0, 120) + "…" : text;
    }
    case "turn_end": {
      const p = record.payload as TurnEnd;
      if (p.duration_ms != null) {
        return `Turn completed (${(p.duration_ms / 1000).toFixed(1)}s)`;
      }
      return "Turn completed";
    }
    case "error": {
      const p = record.payload as ErrorRecord;
      return p.message;
    }
    case "token_usage": {
      const p = record.payload as TokenUsage;
      const parts: string[] = [];
      if (p.input_tokens != null) parts.push(`${p.input_tokens} in`);
      if (p.output_tokens != null) parts.push(`${p.output_tokens} out`);
      return parts.join(" / ") || "Token usage";
    }
    default:
      return "";
  }
}

// ---------------------------------------------------------------------------
// viewRecordColor — color for a ViewRecord (Tokyonight palette)
// ---------------------------------------------------------------------------

const RECORD_TYPE_COLORS: Record<RecordType, string> = {
  user_message: "#7aa2f7",
  assistant_message: "#bb9af7",
  reasoning: "#9ece6a",
  tool_call: "#2ac3de",
  tool_result: "#2ac3de",
  turn_end: "#565f89",
  turn_start: "#565f89",
  token_usage: "#565f89",
  system_event: "#565f89",
  error: "#f7768e",
  session_meta: "#9ece6a",
  context_compaction: "#565f89",
  file_snapshot: "#565f89",
};

export function viewRecordColor(record: ViewRecord): string {
  // Git command risk color override
  if (isGitBashRecord(record)) {
    const cmd = extractBashCommand(record);
    const risk = gitCommandRisk(cmd);
    return GIT_RISK_COLORS[risk];
  }
  return RECORD_TYPE_COLORS[record.record_type] ?? "#565f89";
}

// ---------------------------------------------------------------------------
// isGitBashRecord — detect git bash tool calls
// ---------------------------------------------------------------------------

export function isGitBashRecord(record: ViewRecord): boolean {
  if (record.record_type !== "tool_call") return false;
  const tc = record.payload as ToolCall;
  if (tc.name !== "Bash") return false;
  const cmd = extractBashCommand(record);
  return isGitCommand(cmd);
}

function extractBashCommand(record: ViewRecord): string {
  if (record.record_type !== "tool_call") return "";
  const tc = record.payload as ToolCall;
  if (tc.typed_input?.tool === "bash") {
    return (tc.typed_input as { command?: string }).command ?? "";
  }
  return "";
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function extractText(blocks: ContentBlock[]): string {
  return (
    blocks
      .filter((b) => b.type === "text" && b.text)
      .map((b) => b.text!)
      .join("\n") || ""
  );
}
