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
export interface EventData {
  readonly seq: number;
  readonly ts: number;
  readonly parent_seq: number | null;
  readonly meta?: EventMeta;
  // session.start
  readonly command?: readonly string[];
  readonly pid?: number | null;
  readonly source?: string;
  readonly model?: string;
  readonly agent_type?: string;
  // prompt.submit / message.user.prompt
  readonly text?: string;
  // tool.call / message.assistant.tool_use
  readonly tool?: string;
  readonly args?: Record<string, unknown>;
  readonly tool_use_id?: string;
  // tool.result
  readonly result?: string;
  readonly tool_input?: Record<string, unknown>;
  readonly tool_response?: Record<string, unknown>;
  // file.edit
  readonly path?: string;
  readonly operation?: string;
  // response.complete
  readonly last_assistant_message?: string;
  readonly stop_hook_active?: boolean;
  // error
  readonly message?: string;
  readonly is_interrupt?: boolean;
  // session.end / system.turn.complete
  readonly reason?: string;
  readonly duration_ms?: number;
  // transcript-style fields
  readonly raw?: Record<string, any>;
  readonly content_types?: readonly string[];
  readonly user_type?: string;
  readonly progress_type?: string;
  // transcript envelope (snake_case — converted from camelCase at translate layer)
  readonly cwd?: string;
  readonly session_id?: string;
  // forward-compat: allow unknown fields
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
