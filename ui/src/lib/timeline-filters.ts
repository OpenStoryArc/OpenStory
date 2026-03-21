import type { WireRecord } from "@/types/wire-record";
import type { ToolCall, ToolResult } from "@/types/view-record";

/** Client-side filter predicate: (WireRecord) => boolean.
 *  Mirrors the 21 server-side filters in projection.rs.
 *  Instant switching — no network round trip. */
export type FilterPredicate = (record: WireRecord) => boolean;

// ── Helpers ──────────────────────────────────────────────────────

function isToolCall(r: WireRecord): r is WireRecord & { payload: ToolCall } {
  return r.record_type === "tool_call";
}

function isToolResult(r: WireRecord): r is WireRecord & { payload: ToolResult } {
  return r.record_type === "tool_result";
}

function toolName(r: WireRecord): string {
  if (isToolCall(r)) return r.payload.name;
  return "";
}

function bashCommand(r: WireRecord): string {
  if (!isToolCall(r) || r.payload.name !== "Bash") return "";
  const raw = r.payload.raw_input as Record<string, unknown> | undefined;
  return (raw?.command as string) ?? "";
}

function resultOutput(r: WireRecord): string {
  if (!isToolResult(r)) return "";
  return r.payload.output ?? "";
}

// ── Deep threshold ───────────────────────────────────────────────

const DEEP_DEPTH_THRESHOLD = 2;

// ── Filter definitions ──────────────────────────────────────────
//
// 10 consolidated filters (was 17). Each answers one question.

export const TIMELINE_FILTERS: Record<string, FilterPredicate> = {
  all: () => true,

  // "What did the agent say?" — user prompts + assistant responses
  conversation: (r) =>
    r.record_type === "user_message" || r.record_type === "assistant_message",

  // "What files did it touch?" — reads, edits, writes, searches
  code: (r) =>
    ["Read", "Edit", "Write", "Glob", "Grep"].includes(toolName(r)),

  // "What shell commands ran?" — all bash (excluding git/test which have own filters)
  commands: (r) =>
    toolName(r) === "Bash",

  // "Did the tests pass?" — test commands + pass/fail results
  tests: (r) => {
    const cmd = bashCommand(r);
    if (cmd.includes("cargo test") || cmd.includes("npm test") ||
        cmd.includes("npx vitest") || cmd.includes("npx jest") ||
        cmd.includes("pytest")) return true;
    const out = resultOutput(r);
    if (out.includes("test result:") || out.includes("passed") ||
        out.includes("FAILED") || /\d+\s+failed/.test(out)) return true;
    return false;
  },

  // "What git operations happened?" — git commands + results
  git: (r) => {
    if (bashCommand(r).includes("git ")) return true;
    // Also match results of git commands (heuristic: output mentions commit/branch/merge)
    const out = resultOutput(r);
    if (out && (out.includes("commit ") || out.includes("branch ") ||
        out.startsWith("[") && out.includes("]"))) return true;
    return false;
  },

  // "What went wrong?" — errors, compile errors, failed tool results
  errors: (r) => {
    if (r.record_type === "error") return true;
    if (isToolResult(r) && r.payload.is_error) return true;
    const out = resultOutput(r);
    if (out.includes("error[E") || out.includes("error[") ||
        out.includes("TS2") || out.includes("TS1") ||
        out.includes("SyntaxError")) return true;
    return false;
  },

  // "What is the agent thinking?" — extended reasoning
  thinking: (r) => r.record_type === "reasoning",

  // "Did planning happen?" — EnterPlanMode / ExitPlanMode tool calls
  plans: (r) =>
    toolName(r) === "EnterPlanMode" || toolName(r) === "ExitPlanMode",

  // "What was delegated?" — subagent calls + nested operations
  agents: (r) => toolName(r) === "Agent" || r.depth >= DEEP_DEPTH_THRESHOLD,
};

/** Flat filter list — one row, no groups. */
export interface FilterGroup {
  readonly label: string;
  readonly filters: readonly string[];
}

export const FILTER_GROUPS: readonly FilterGroup[] = [
  { label: "", filters: ["all", "conversation", "code", "commands", "tests", "git", "errors", "thinking", "plans", "agents"] },
];
