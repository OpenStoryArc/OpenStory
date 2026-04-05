/** CloudEvents 1.0 envelope with open-story extensions */
export interface CloudEvent {
  readonly specversion: "1.0";
  readonly id: string;
  readonly source: string;
  readonly type: ArcEventType;
  readonly time: string;
  readonly datacontenttype: string;
  readonly data: EventData;
  readonly subtype?: string;
  readonly subject?: string;
  readonly dataschema?: string;
}

export type ArcEventType =
  // Unified type (primary — all new events use this)
  | "io.arc.event"
  // Legacy hook-style types (for persisted data backward compat)
  | "io.arc.session.start"
  | "io.arc.session.end"
  | "io.arc.prompt.submit"
  | "io.arc.response.complete"
  | "io.arc.tool.call"
  | "io.arc.tool.result"
  | "io.arc.file.edit"
  | "io.arc.error"
  // Legacy transcript-style types (for persisted data backward compat)
  | "io.arc.transcript.user"
  | "io.arc.transcript.assistant"
  | "io.arc.transcript.system"
  | "io.arc.transcript.progress"
  | "io.arc.transcript.snapshot"
  | "io.arc.transcript.queue";

export interface EventMeta {
  readonly hook: string;
  readonly cwd?: string;
  readonly permission_mode?: string;
  readonly transcript_path?: string;
  readonly tool?: string;
}

/* eslint-disable @typescript-eslint/no-explicit-any */

/** Monadic EventData: foundation + optional typed agent payload. */
export interface EventData {
  // ── Foundation (always present) ──
  readonly raw: Record<string, any>;
  readonly seq: number;
  readonly session_id: string;
  // ── The lift: agent-specific payload, absent for unknown agents ──
  readonly agent_payload?: AgentPayload;
  // forward-compat
  [key: string]: unknown;
}

/** Tagged union of agent-specific payloads. Dispatch on meta.agent. */
export type AgentPayload = ClaudeCodePayload | PiMonoPayload;

interface PayloadMeta {
  readonly agent: string;
}

export interface ClaudeCodePayload {
  readonly _variant: "claude-code";
  readonly meta: PayloadMeta;
  readonly uuid?: string;
  readonly parent_uuid?: string;
  readonly cwd?: string;
  readonly timestamp?: string;
  readonly version?: string;
  readonly text?: string;
  readonly model?: string;
  readonly stop_reason?: any;
  readonly content_types?: readonly string[];
  readonly tool?: string;
  readonly args?: Record<string, unknown>;
  readonly token_usage?: Record<string, any>;
  readonly slug?: string;
  readonly message_id?: string;
  readonly git_branch?: string;
  readonly is_sidechain?: boolean;
  readonly agent_id?: string;
  readonly user_type?: string;
  readonly progress_type?: string;
  readonly parent_tool_use_id?: string;
  readonly operation?: string;
  readonly hook_count?: number;
  readonly prevented_continuation?: boolean;
  readonly duration_ms?: number;
  [key: string]: unknown;
}

export interface PiMonoPayload {
  readonly _variant: "pi-mono";
  readonly meta: PayloadMeta;
  readonly uuid?: string;
  readonly parent_uuid?: string;
  readonly cwd?: string;
  readonly timestamp?: string;
  readonly version?: any;
  readonly text?: string;
  readonly model?: string;
  readonly stop_reason?: string;
  readonly content_types?: readonly string[];
  readonly tool?: string;
  readonly args?: Record<string, unknown>;
  readonly token_usage?: Record<string, any>;
  readonly provider?: string;
  readonly thinking_level?: string;
  readonly model_id?: string;
  readonly tool_call_id?: string;
  readonly tool_name?: string;
  readonly is_error?: boolean;
  readonly command?: string;
  readonly exit_code?: any;
  readonly output?: string;
  readonly summary?: string;
  readonly tokens_before?: number;
  readonly first_kept_entry_id?: string;
  [key: string]: unknown;
}
/* eslint-enable @typescript-eslint/no-explicit-any */

/** Derive a short label from event type + subtype */
export function eventLabel(type: ArcEventType, subtype?: string): string {
  // Unified type: derive from subtype
  if (type === "io.arc.event" && subtype) {
    return SUBTYPE_LABELS[subtype] ?? subtype.split(".").pop() ?? subtype;
  }
  return LEGACY_TYPE_LABELS[type] ?? type;
}

/** Subtype → short label mapping for io.arc.event */
const SUBTYPE_LABELS: Record<string, string> = {
  "message.user.prompt": "Prompt",
  "message.user.tool_result": "Result",
  "message.assistant.text": "Response",
  "message.assistant.tool_use": "Tool Use",
  "message.assistant.thinking": "Thinking",
  "system.turn.complete": "Complete",
  "system.error": "Error",
  "system.compact": "Compact",
  "system.hook": "Hook",
  "system.session.start": "Start",
  "system.session.end": "End",
  "progress.bash": "Bash",
  "progress.agent": "Agent",
  "progress.hook": "Hook",
  "file.snapshot": "Snapshot",
  "file.edit": "Edit",
  "queue.enqueue": "Enqueue",
  "queue.dequeue": "Dequeue",
};

/** Legacy type → short label (backward compat for persisted events) */
const LEGACY_TYPE_LABELS: Record<string, string> = {
  "io.arc.session.start": "Start",
  "io.arc.session.end": "End",
  "io.arc.prompt.submit": "Prompt",
  "io.arc.response.complete": "Response",
  "io.arc.tool.call": "Call",
  "io.arc.tool.result": "Result",
  "io.arc.file.edit": "Edit",
  "io.arc.error": "Error",
  "io.arc.transcript.user": "User",
  "io.arc.transcript.assistant": "Assistant",
  "io.arc.transcript.system": "System",
  "io.arc.transcript.progress": "Progress",
  "io.arc.transcript.snapshot": "Snapshot",
  "io.arc.transcript.queue": "Queue",
};

/** Short label for event type display (legacy export) */
export const EVENT_TYPE_LABELS: Record<string, string> = {
  ...LEGACY_TYPE_LABELS,
  ...Object.fromEntries(
    Object.entries(SUBTYPE_LABELS).map(([k, v]) => [`io.arc.event:${k}`, v])
  ),
};

/** Category grouping for stat cards */
export type EventCategory =
  | "prompts"
  | "responses"
  | "errors"
  | "tools"
  | "files";

/** Classify an event into a stat category (subtype-aware) */
export function eventCategory(type: ArcEventType, subtype?: string): EventCategory | null {
  // Unified type: classify by hierarchical subtype
  if (type === "io.arc.event") {
    if (!subtype) return null;
    if (subtype === "message.user.prompt") return "prompts";
    if (subtype === "message.user.tool_result") return "tools";
    if (subtype === "message.assistant.tool_use") return "tools";
    if (subtype.startsWith("message.assistant.")) return "responses";
    if (subtype === "system.error") return "errors";
    if (subtype.startsWith("file.")) return "files";
    return null;
  }

  // Legacy hook-style types
  switch (type) {
    case "io.arc.prompt.submit":
      return "prompts";
    case "io.arc.response.complete":
      return "responses";
    case "io.arc.error":
      return "errors";
    case "io.arc.tool.call":
    case "io.arc.tool.result":
      return "tools";
    case "io.arc.file.edit":
      return "files";
    // Legacy transcript-style types (subtype-dependent)
    case "io.arc.transcript.user":
      return subtype === "tool_result" ? "tools" : "prompts";
    case "io.arc.transcript.assistant":
      if (subtype === "tool_use") return "tools";
      return "responses";
    case "io.arc.transcript.system":
      if (subtype?.includes("error")) return "errors";
      return null;
    default:
      return null;
  }
}
