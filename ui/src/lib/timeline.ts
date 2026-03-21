/**
 * Pure transform: ViewRecord events → TimelineRow[]
 *
 * Flattens all sessions' events into a single sorted timeline.
 * This is the foundation of the UI — events in, rows out.
 */

import type {
  ViewRecord,
  AssistantMessage,
  ContentBlock,
  ErrorRecord,
  Reasoning,
  ToolCall,
  ToolResult,
  UserMessage,
  SystemEvent,
  TurnEnd,
} from "@/types/view-record";
import { toolInputSummary } from "@/types/view-record";
import type { WireRecord } from "@/types/wire-record";
import { stripAnsi } from "@/lib/strip-ansi";

export type TimelineCategory =
  | "prompt"
  | "response"
  | "tool"
  | "result"
  | "thinking"
  | "system"
  | "error"
  | "turn";

export interface TimelineRow {
  readonly id: string;
  readonly timestamp: string;
  readonly sessionId: string;
  readonly category: TimelineCategory;
  readonly toolName: string;
  readonly summary: string;
  readonly record: ViewRecord | WireRecord;
  /** File path hint from parent tool_call (for syntax highlighting results). */
  readonly fileHint?: string;
}

/** Record types we skip — noise in the timeline */
const SKIP_TYPES = new Set(["token_usage", "session_meta", "turn_start", "file_snapshot", "context_compaction"]);

/** Max summary length — generous since cards auto-size to content */
const MAX_SUMMARY = 500;

function truncate(s: string, max: number): string {
  const clean = stripAnsi(s);
  if (clean.length <= max) return clean;
  return clean.slice(0, max - 1) + "\u2026";
}

function extractTextFromBlocks(blocks: ContentBlock[]): string {
  for (const b of blocks) {
    if (b.type === "text" && b.text) return b.text;
  }
  return "";
}

function recordToRow(r: ViewRecord): TimelineRow | null {
  if (SKIP_TYPES.has(r.record_type)) return null;

  const base = { id: r.id, timestamp: r.timestamp, sessionId: r.session_id, toolName: "", record: r };

  switch (r.record_type) {
    case "user_message": {
      const p = r.payload as UserMessage;
      const text = typeof p.content === "string"
        ? p.content
        : extractTextFromBlocks(p.content);
      return { ...base, category: "prompt", summary: truncate(text || "User message", MAX_SUMMARY) };
    }
    case "assistant_message": {
      const p = r.payload as AssistantMessage;
      const text = extractTextFromBlocks(p.content);
      return { ...base, category: "response", summary: truncate(text || "Assistant response", MAX_SUMMARY) };
    }
    case "reasoning": {
      const p = r.payload as Reasoning;
      const text = p.summary.length > 0 ? p.summary[0]! : p.content ?? "Thinking...";
      return { ...base, category: "thinking", summary: truncate(text, MAX_SUMMARY) };
    }
    case "tool_call": {
      const p = r.payload as ToolCall;
      const detail = toolInputSummary(p.typed_input);
      return { ...base, category: "tool", toolName: p.name, summary: truncate(detail || p.name, MAX_SUMMARY) };
    }
    case "tool_result": {
      const p = r.payload as ToolResult;
      if (p.is_error) {
        return { ...base, category: "error", summary: truncate(p.output ?? "Tool error", MAX_SUMMARY) };
      }
      return { ...base, category: "result", summary: truncate(p.output ?? "Result", MAX_SUMMARY) };
    }
    case "error": {
      const p = r.payload as ErrorRecord;
      return { ...base, category: "error", summary: truncate(p.message, MAX_SUMMARY) };
    }
    case "system_event": {
      const p = r.payload as SystemEvent;
      const msg = p.message ?? p.subtype;
      return { ...base, category: "system", summary: truncate(msg, MAX_SUMMARY) };
    }
    case "turn_end": {
      const p = r.payload as TurnEnd;
      const dur = p.duration_ms != null ? ` ${formatMs(p.duration_ms)}` : "";
      return { ...base, category: "turn", summary: `Turn complete${dur}` };
    }
    default:
      return null;
  }
}

function formatMs(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  const mins = Math.floor(ms / 60000);
  const secs = Math.round((ms % 60000) / 1000);
  return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
}

/**
 * Transform a flat array of records into sorted timeline rows.
 *
 * Pure function: records in → rows out.
 * Accepts both ViewRecord and WireRecord (WireRecord is a superset).
 * Records are assumed to be roughly sorted by timestamp already (from server),
 * but we sort again to handle out-of-order live appends.
 */
export function toTimelineRows(
  records: readonly (ViewRecord | WireRecord)[],
): TimelineRow[] {
  const rows: TimelineRow[] = [];

  // Track most recent tool_call file path for linking to results
  let lastToolFilePath: string | undefined;

  for (const r of records) {
    // Track file path from tool_calls
    if (r.record_type === "tool_call") {
      const payload = r.payload as ToolCall;
      const ti = payload.typed_input;
      if (ti) {
        const fp = (ti as Record<string, unknown>).file_path as string | undefined;
        lastToolFilePath = fp ?? undefined;
      } else {
        lastToolFilePath = undefined;
      }
    }

    const row = recordToRow(r);
    if (row) {
      // Attach file hint to tool_result rows
      if (row.category === "result" && lastToolFilePath) {
        rows.push({ ...row, fileHint: lastToolFilePath });
      } else {
        rows.push(row);
      }
    }

    // Clear file hint after consuming result
    if (r.record_type === "tool_result") {
      lastToolFilePath = undefined;
    }
  }

  rows.sort((a, b) => b.timestamp.localeCompare(a.timestamp));
  return rows;
}
