import type { CloudEvent } from "@/types/cloud-event";
import { isGitCommand, gitCommandRisk, gitCommandSummary, GIT_RISK_COLORS } from "@/lib/git-commands";

/** Color mapping for subtypes and legacy types (Tokyonight palette) */
const SUBTYPE_COLORS: Record<string, string> = {
  // Unified subtypes
  "message.user.prompt": "#7aa2f7",
  "message.user.tool_result": "#2ac3de",
  "message.assistant.text": "#bb9af7",
  "message.assistant.tool_use": "#2ac3de",
  "message.assistant.thinking": "#9ece6a",
  "system.turn.complete": "#565f89",
  "system.error": "#f7768e",
  "system.compact": "#565f89",
  "system.hook": "#565f89",
  "system.session.start": "#9ece6a",
  "system.session.end": "#565f89",
  "progress.bash": "#e0af68",
  "progress.agent": "#e0af68",
  "progress.hook": "#e0af68",
  "file.snapshot": "#565f89",
  "file.edit": "#e0af68",
  "queue.enqueue": "#565f89",
  "queue.dequeue": "#565f89",
};

/** Legacy type → color (backward compat for persisted events) */
const LEGACY_TYPE_COLORS: Record<string, string> = {
  "io.arc.session.start": "#9ece6a",
  "io.arc.session.end": "#565f89",
  "io.arc.prompt.submit": "#7aa2f7",
  "io.arc.response.complete": "#bb9af7",
  "io.arc.tool.call": "#2ac3de",
  "io.arc.tool.result": "#2ac3de",
  "io.arc.file.edit": "#e0af68",
  "io.arc.error": "#f7768e",
  "io.arc.transcript.user": "#7aa2f7",
  "io.arc.transcript.assistant": "#bb9af7",
  "io.arc.transcript.system": "#565f89",
  "io.arc.transcript.progress": "#e0af68",
  "io.arc.transcript.snapshot": "#565f89",
  "io.arc.transcript.queue": "#565f89",
};

/** Check if an event is a git bash event */
export function isGitBashEvent(event: CloudEvent): boolean {
  const tool = (event.data?.tool as string) ?? "";
  if (tool !== "Bash") return false;
  const cmd = ((event.data?.args as Record<string, unknown>)?.command as string) ?? "";
  if (!isGitCommand(cmd)) return false;

  // Unified format
  if (event.type === "io.arc.event" && event.subtype === "message.assistant.tool_use") return true;
  // Legacy format
  if (event.type === "io.arc.tool.call") return true;
  return false;
}

/** Get color for an event */
export function eventColor(event: CloudEvent): string {
  // Git command risk color override
  if (isGitBashEvent(event)) {
    const cmd = ((event.data?.args as Record<string, unknown>)?.command as string) ?? "";
    const risk = gitCommandRisk(cmd);
    return GIT_RISK_COLORS[risk];
  }

  if (event.type === "io.arc.event" && event.subtype) {
    return SUBTYPE_COLORS[event.subtype] ?? "#565f89";
  }
  return LEGACY_TYPE_COLORS[event.type] ?? "#565f89";
}

/** Legacy TYPE_COLORS export — still works for backward-compat consumers */
export const TYPE_COLORS: Record<string, string> = {
  ...LEGACY_TYPE_COLORS,
};

/** Extract text content from a transcript event's raw message */
function extractTranscriptText(event: CloudEvent): string {
  // New format: top-level data.text
  if (event.data.text && typeof event.data.text === "string") {
    return event.data.text;
  }
  // Legacy: dig into raw
  const raw = event.data.raw;
  if (!raw) return "";
  const message = raw.message;
  if (!message) return "";
  const content = message.content;
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    for (const block of content) {
      if (block.type === "text" && block.text) {
        return block.text;
      }
    }
  }
  return "";
}

/** Extract tool names from an event */
function extractTranscriptToolNames(event: CloudEvent): string[] {
  // New format: top-level data.tool
  if (event.data.tool && typeof event.data.tool === "string") {
    return [event.data.tool as string];
  }
  // Legacy: dig into raw content blocks
  const raw = event.data.raw;
  if (!raw) return [];
  const content = raw.message?.content;
  if (!Array.isArray(content)) return [];
  return content
    .filter((b: Record<string, unknown>) => b.type === "tool_use" && b.name)
    .map((b: Record<string, unknown>) => b.name as string);
}

/** Get a one-line summary for an event */
export function eventSummary(event: CloudEvent): string {
  // Unified io.arc.event type — dispatch on subtype
  if (event.type === "io.arc.event") {
    return arcEventSummary(event);
  }

  // Legacy event types
  switch (event.type) {
    case "io.arc.session.start":
      return `Session started${event.data.model ? ` (${event.data.model})` : ""}`;
    case "io.arc.session.end":
      return `Session ended: ${event.data.reason ?? "unknown"}`;
    case "io.arc.prompt.submit":
      return truncate(event.data.text ?? "", 120);
    case "io.arc.response.complete":
      return truncate(event.data.last_assistant_message ?? "", 120);
    case "io.arc.tool.call": {
      const tool = event.subtype ?? event.data.tool ?? "unknown";
      const detail = toolCallDetail(event);
      return detail ? `${tool}: ${detail}` : tool;
    }
    case "io.arc.tool.result": {
      const tool = event.data.tool ?? "unknown";
      return `${tool} result (${truncate(event.data.result ?? "", 80)})`;
    }
    case "io.arc.file.edit":
      return `${event.data.operation ?? "edit"}: ${basename(event.data.path ?? "")}`;
    case "io.arc.error":
      return truncate(event.data.message ?? "Error", 120);

    // Legacy transcript-style events
    case "io.arc.transcript.user": {
      if (event.subtype === "tool_result") return "Tool result";
      const text = extractTranscriptText(event);
      return text ? truncate(text, 120) : "User message";
    }
    case "io.arc.transcript.assistant": {
      if (event.subtype === "tool_use") {
        const tools = extractTranscriptToolNames(event);
        return tools.length > 0 ? tools.join(", ") : "Tool use";
      }
      if (event.subtype === "thinking") return "Thinking...";
      const text = extractTranscriptText(event);
      return text ? truncate(text, 120) : "Assistant response";
    }
    case "io.arc.transcript.system": {
      if (event.subtype === "turn_duration") {
        const ms = event.data.duration_ms;
        return ms != null ? `Turn completed (${formatDurationCompact(ms)})` : "Turn completed";
      }
      return event.subtype ?? "System";
    }
    case "io.arc.transcript.progress":
      return event.subtype ?? event.data.progress_type ?? "Progress";
    case "io.arc.transcript.snapshot":
      return "File history snapshot";
    case "io.arc.transcript.queue":
      return event.data.operation ?? event.subtype ?? "Queue operation";

    default:
      return event.type;
  }
}

/** Summary for unified io.arc.event type */
function arcEventSummary(event: CloudEvent): string {
  const sub = event.subtype ?? "";

  if (sub === "message.user.prompt") {
    const text = extractTranscriptText(event);
    return text ? truncate(text, 120) : "User message";
  }
  if (sub === "message.user.tool_result") {
    return "Tool result";
  }
  if (sub === "message.assistant.tool_use") {
    const tools = extractTranscriptToolNames(event);
    if (tools.length > 0) {
      const detail = toolCallDetail(event);
      return detail ? `${tools[0]}: ${detail}` : tools.join(", ");
    }
    return "Tool use";
  }
  if (sub === "message.assistant.thinking") {
    return "Thinking...";
  }
  if (sub === "message.assistant.text") {
    const text = extractTranscriptText(event);
    return text ? truncate(text, 120) : "Assistant response";
  }
  if (sub === "system.turn.complete") {
    const ms = event.data.duration_ms;
    return ms != null ? `Turn completed (${formatDurationCompact(ms as number)})` : "Turn completed";
  }
  if (sub === "system.error") {
    return truncate((event.data.message as string) ?? "Error", 120);
  }
  if (sub === "system.hook") {
    return "Hook summary";
  }
  if (sub === "system.compact") {
    return "Context compacted";
  }
  if (sub.startsWith("progress.")) {
    return sub.split(".").pop() ?? "Progress";
  }
  if (sub === "file.snapshot") {
    return "File history snapshot";
  }
  if (sub === "file.edit") {
    return `${event.data.operation ?? "edit"}: ${basename((event.data.path as string) ?? "")}`;
  }
  if (sub.startsWith("queue.")) {
    return event.data.operation ?? sub.split(".").pop() ?? "Queue operation";
  }

  return sub || event.type;
}

/** Format ms as compact duration */
function formatDurationCompact(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  const mins = Math.floor(ms / 60000);
  const secs = Math.round((ms % 60000) / 1000);
  return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
}

/** Extract a compact detail string for tool calls */
function toolCallDetail(event: CloudEvent): string {
  const args = event.data.args;
  if (!args) return "";

  // Common patterns
  if (typeof args["file_path"] === "string") return basename(args["file_path"]);
  if (typeof args["command"] === "string") {
    const cmd = args["command"] as string;
    // Use git summary for git commands
    if (isGitCommand(cmd)) return truncate(gitCommandSummary(cmd), 80);
    return truncate(shortenCommand(cmd), 80);
  }
  if (typeof args["pattern"] === "string") return truncate(args["pattern"], 60);
  if (typeof args["query"] === "string") return truncate(args["query"], 60);
  if (typeof args["url"] === "string") return truncate(args["url"], 80);

  return "";
}

/** Strip `cd <absolute-path> &&` prefix from shell commands */
export function shortenCommand(cmd: string): string {
  if (!cmd) return cmd;
  // Match: cd <absolute-path> && <rest>
  // Absolute paths start with / or a drive letter (C:\)
  const match = cmd.match(/^cd\s+(?:"[^"]+"|'[^']+'|\/\S+|[A-Za-z]:\\\S+)\s+&&\s+(.+)$/);
  return match ? match[1]! : cmd;
}

/** Extract basename from a file path */
export function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

/** Truncate string with ellipsis */
export function truncate(str: string, max: number): string {
  if (str.length <= max) return str;
  return str.slice(0, max - 1) + "\u2026";
}

/** Status badge colors */
export const STATUS_COLORS: Record<string, string> = {
  ongoing: "#9ece6a",
  completed: "#565f89",
  errored: "#f7768e",
  stale: "#e0af68",
};

/** File operation badge colors */
export const OP_COLORS = {
  create: "#9ece6a",
  modify: "#e0af68",
} as const;
