/** Session summary from REST API / WebSocket session_list */
export interface SessionSummary {
  readonly session_id: string;
  readonly status: "ongoing" | "completed" | "errored" | "stale";
  readonly start_time: string;
  readonly event_count: number;
  readonly tool_calls?: number;
  readonly files_edited?: number;
  readonly model?: string;
  readonly duration_ms?: number;
  readonly first_prompt?: string;
  readonly cwd?: string;
  readonly project_id?: string;
  readonly project_name?: string;
}

/** Activity summary from /api/sessions/{id}/activity */
export interface ActivitySummary {
  readonly first_prompt: string;
  readonly files_touched: readonly FileTouch[];
  readonly tool_breakdown: Record<string, number>;
  readonly error_messages: readonly string[];
  readonly last_response: string;
  readonly conversation_turns: number;
  readonly plan_count: number;
  readonly duration_ms: number;
  readonly start_time: string;
}

export interface FileTouch {
  readonly path: string;
  readonly operation: "create" | "modify";
}

/** Transcript entry from /api/sessions/{id}/transcript */
export type TranscriptEntry =
  | TextEntry
  | ToolUseEntry
  | ToolResultEntry
  | ThinkingEntry;

export interface TextEntry {
  readonly kind: "text";
  readonly role: "user" | "assistant";
  readonly text: string;
  readonly model?: string;
  readonly timestamp?: string;
}

export interface ToolUseEntry {
  readonly kind: "tool_use";
  readonly tool_name: string;
  readonly tool_use_id: string;
  readonly input: Record<string, unknown>;
}

export interface ToolResultEntry {
  readonly kind: "tool_result";
  readonly tool_use_id: string;
  readonly text: string;
}

export interface ThinkingEntry {
  readonly kind: "thinking";
  readonly text: string;
}

/** Plan from /api/plans */
export interface PlanSummary {
  readonly id: string;
  readonly session_id: string;
  readonly title: string;
  readonly timestamp: string;
}

export interface PlanDetail extends PlanSummary {
  readonly content: string;
}

/** Tool schema from /api/tool-schemas */
export interface ToolSchema {
  readonly display_formatter: string;
  readonly display_fields: readonly string[];
  readonly fields: readonly ToolField[];
}

export interface ToolField {
  readonly name: string;
  readonly required: boolean;
  readonly description: string;
}
