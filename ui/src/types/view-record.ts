/** Typed view records from the open-story-views crate.
 *  These mirror the Rust types and replace the untyped CloudEvent data bag. */

// ---------------------------------------------------------------------------
// ViewRecord — the top-level wrapper
// ---------------------------------------------------------------------------

export interface ViewRecord {
  readonly id: string;
  readonly seq: number;
  readonly session_id: string;
  readonly timestamp: string;
  readonly record_type: RecordType;
  readonly payload: RecordPayload;
  /** Subagent identity: which agent produced this event (null = main agent). */
  readonly agent_id: string | null;
  /** Whether this event belongs to a sidechain (subagent file). */
  readonly is_sidechain: boolean;
}

export type RecordType =
  | "session_meta"
  | "turn_start"
  | "turn_end"
  | "user_message"
  | "assistant_message"
  | "reasoning"
  | "tool_call"
  | "tool_result"
  | "token_usage"
  | "context_compaction"
  | "file_snapshot"
  | "system_event"
  | "error";

export type RecordPayload =
  | SessionMeta
  | TurnStart
  | TurnEnd
  | UserMessage
  | AssistantMessage
  | Reasoning
  | ToolCall
  | ToolResult
  | TokenUsage
  | ContextCompaction
  | FileSnapshot
  | SystemEvent
  | ErrorRecord;

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

export interface SessionMeta {
  readonly cwd: string;
  readonly model: string;
  readonly version: string;
  readonly git?: { branch?: string; commit?: string };
}

export interface TurnStart {
  readonly turn_id?: string;
}

export interface TurnEnd {
  readonly turn_id?: string;
  readonly reason?: string;
  readonly duration_ms?: number;
}

export interface UserMessage {
  readonly content: string | ContentBlock[];
  readonly images?: unknown[];
}

export interface AssistantMessage {
  readonly model: string;
  readonly content: ContentBlock[];
  readonly stop_reason?: string;
  readonly end_turn?: boolean;
  readonly phase?: string;
}

export interface ContentBlock {
  readonly type: "text" | "code_block" | "image";
  readonly text?: string;
  readonly language?: string;
  readonly source?: unknown;
}

export interface Reasoning {
  readonly summary: string[];
  readonly content?: string;
  readonly encrypted: boolean;
}

export interface ToolCall {
  readonly call_id: string;
  readonly name: string;
  readonly input: unknown;
  readonly raw_input: unknown;
  readonly typed_input?: ToolInput;
  readonly status?: string;
}

export interface ToolResult {
  readonly call_id: string;
  readonly output?: string;
  readonly is_error: boolean;
}

export interface TokenUsage {
  readonly input_tokens?: number;
  readonly output_tokens?: number;
  readonly total_tokens?: number;
  readonly scope: "turn" | "session_total";
}

export interface ContextCompaction {
  readonly reason?: string;
  readonly message?: string;
}

export interface FileSnapshot {
  readonly git_commit?: string;
  readonly git_message?: string;
  readonly tracked_files?: unknown;
}

export interface SystemEvent {
  readonly subtype: string;
  readonly message?: string;
  readonly duration_ms?: number;
}

export interface ErrorRecord {
  readonly code: string;
  readonly message: string;
  readonly details?: string;
}

// ---------------------------------------------------------------------------
// ToolInput — discriminated union for all Claude Code tools
// ---------------------------------------------------------------------------

export type ToolInput =
  | ReadInput
  | EditInput
  | WriteInput
  | GlobInput
  | GrepInput
  | NotebookEditInput
  | BashInput
  | WebFetchInput
  | WebSearchInput
  | AgentInput
  | TaskCreateInput
  | TaskUpdateInput
  | TaskGetInput
  | TaskListInput
  | TaskOutputInput
  | TaskStopInput
  | EnterPlanModeInput
  | ExitPlanModeInput
  | EnterWorktreeInput
  | SkillInput
  | AskUserQuestionInput
  | LspInput
  | ToolSearchInput
  | CronCreateInput
  | CronDeleteInput
  | CronListInput
  | UnknownToolInput;

export interface ReadInput {
  readonly tool: "read";
  readonly file_path: string;
  readonly offset?: number;
  readonly limit?: number;
  readonly pages?: string;
}

export interface EditInput {
  readonly tool: "edit";
  readonly file_path: string;
  readonly old_string: string;
  readonly new_string: string;
  readonly replace_all?: boolean;
}

export interface WriteInput {
  readonly tool: "write";
  readonly file_path: string;
  readonly content: string;
}

export interface GlobInput {
  readonly tool: "glob";
  readonly pattern: string;
  readonly path?: string;
}

export interface GrepInput {
  readonly tool: "grep";
  readonly pattern: string;
  readonly path?: string;
  readonly glob?: string;
  readonly output_mode?: string;
}

export interface NotebookEditInput {
  readonly tool: "notebook_edit";
  readonly notebook_path: string;
  readonly new_source: string;
  readonly cell_id?: string;
  readonly cell_type?: string;
  readonly edit_mode?: string;
}

export interface BashInput {
  readonly tool: "bash";
  readonly command: string;
  readonly description?: string;
  readonly timeout?: number;
  readonly run_in_background?: boolean;
}

export interface WebFetchInput {
  readonly tool: "web_fetch";
  readonly url: string;
  readonly prompt?: string;
}

export interface WebSearchInput {
  readonly tool: "web_search";
  readonly query: string;
}

export interface AgentInput {
  readonly tool: "agent";
  readonly prompt: string;
  readonly subagent_type?: string;
  readonly description?: string;
  readonly isolation?: string;
  readonly run_in_background?: boolean;
  readonly resume?: string;
}

export interface TaskCreateInput {
  readonly tool: "task_create";
  readonly subject: string;
  readonly description: string;
}

export interface TaskUpdateInput {
  readonly tool: "task_update";
  readonly task_id: string;
  readonly status?: string;
  readonly subject?: string;
}

export interface TaskGetInput {
  readonly tool: "task_get";
  readonly task_id: string;
}

export interface TaskListInput {
  readonly tool: "task_list";
}

export interface TaskOutputInput {
  readonly tool: "task_output";
  readonly task_id: string;
  readonly block?: boolean;
  readonly timeout?: number;
}

export interface TaskStopInput {
  readonly tool: "task_stop";
  readonly task_id?: string;
}

export interface EnterPlanModeInput {
  readonly tool: "enter_plan_mode";
}

export interface ExitPlanModeInput {
  readonly tool: "exit_plan_mode";
  readonly plan?: string;
}

export interface EnterWorktreeInput {
  readonly tool: "enter_worktree";
  readonly name?: string;
}

export interface SkillInput {
  readonly tool: "skill";
  readonly skill: string;
  readonly args?: string;
}

export interface AskUserQuestionInput {
  readonly tool: "ask_user_question";
  readonly question: unknown;
}

export interface LspInput {
  readonly tool: "lsp";
  readonly [key: string]: unknown;
}

export interface ToolSearchInput {
  readonly tool: "tool_search";
  readonly query: string;
  readonly max_results?: number;
}

export interface CronCreateInput {
  readonly tool: "cron_create";
  readonly schedule: string;
  readonly command: string;
}

export interface CronDeleteInput {
  readonly tool: "cron_delete";
  readonly id: string;
}

export interface CronListInput {
  readonly tool: "cron_list";
}

export interface UnknownToolInput {
  readonly tool: "unknown";
  readonly name: string;
  readonly raw: unknown;
}

// ---------------------------------------------------------------------------
// ConversationEntry — from /api/sessions/{id}/conversation
// ---------------------------------------------------------------------------

export interface PairedConversation {
  readonly entries: ConversationEntry[];
}

export type ConversationEntry =
  | UserMessageEntry
  | AssistantMessageEntry
  | ReasoningEntry
  | ToolRoundtripEntry
  | TokenUsageEntry
  | SystemEntry;

export interface UserMessageEntry {
  readonly entry_type: "user_message";
  readonly id: string;
  readonly seq: number;
  readonly session_id: string;
  readonly timestamp: string;
  readonly record_type: "user_message";
  readonly payload: UserMessage;
}

export interface AssistantMessageEntry {
  readonly entry_type: "assistant_message";
  readonly id: string;
  readonly seq: number;
  readonly session_id: string;
  readonly timestamp: string;
  readonly record_type: "assistant_message";
  readonly payload: AssistantMessage;
}

export interface ReasoningEntry {
  readonly entry_type: "reasoning";
  readonly id: string;
  readonly seq: number;
  readonly session_id: string;
  readonly timestamp: string;
  readonly record_type: "reasoning";
  readonly payload: Reasoning;
}

export interface ToolRoundtripEntry {
  readonly entry_type: "tool_roundtrip";
  readonly call: ViewRecord;
  readonly result: ViewRecord | null;
}

export interface TokenUsageEntry {
  readonly entry_type: "token_usage";
  readonly id: string;
  readonly seq: number;
  readonly session_id: string;
  readonly timestamp: string;
  readonly record_type: "token_usage";
  readonly payload: TokenUsage;
}

export interface SystemEntry {
  readonly entry_type: "system";
  readonly id: string;
  readonly seq: number;
  readonly session_id: string;
  readonly timestamp: string;
  readonly record_type: string;
  readonly payload: unknown;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Type guard: is this a tool_call ViewRecord? */
export function isToolCall(record: ViewRecord): record is ViewRecord & { payload: ToolCall } {
  return record.record_type === "tool_call";
}

/** Type guard: is this a ToolRoundtripEntry? */
export function isToolRoundtrip(entry: ConversationEntry): entry is ToolRoundtripEntry {
  return entry.entry_type === "tool_roundtrip";
}

/** Extract a compact detail string from a typed ToolInput */
export function toolInputSummary(input: ToolInput | undefined): string {
  if (!input) return "";
  switch (input.tool) {
    case "read":
    case "edit":
    case "write":
      return input.file_path;
    case "bash":
      return input.command;
    case "grep":
    case "glob":
      return input.pattern;
    case "web_search":
      return input.query;
    case "web_fetch":
      return input.url;
    case "agent":
      return input.description ?? input.prompt;
    case "skill":
      return input.skill;
    case "notebook_edit":
      return input.notebook_path;
    default:
      return "";
  }
}
