import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { viewRecordLabel, viewRecordSummary } from "@/lib/view-record-transforms";
import type { ViewRecord, ToolCall, ToolResult } from "@/types/view-record";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function makeRecord(
  recordType: string,
  payload: unknown,
  overrides?: Partial<ViewRecord>,
): ViewRecord {
  return {
    id: "evt-1",
    seq: 1,
    session_id: "sess-1",
    timestamp: "2025-01-09T10:00:00Z",
    record_type: recordType as ViewRecord["record_type"],
    payload: payload as ViewRecord["payload"],
    agent_id: null,
    is_sidechain: false,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// describe("viewRecordLabel")
// ---------------------------------------------------------------------------
describe("viewRecordLabel", () => {
  const cases: [string, ViewRecord["record_type"], string][] = [
    ["user_message", "user_message", "Prompt"],
    ["assistant_message", "assistant_message", "Response"],
    ["reasoning", "reasoning", "Thinking"],
    ["tool_call", "tool_call", "Tool Use"],
    ["tool_result", "tool_result", "Result"],
    ["turn_end", "turn_end", "Complete"],
    ["token_usage", "token_usage", "Tokens"],
    ["system_event", "system_event", "System"],
    ["error", "error", "Error"],
    ["session_meta", "session_meta", "Start"],
    ["context_compaction", "context_compaction", "Compact"],
    ["file_snapshot", "file_snapshot", "Snapshot"],
  ];

  it.each(cases)("should return '%s' → '%s'", (_label, recordType, expected) => {
    expect(viewRecordLabel(recordType)).toBe(expected);
  });
});

// ---------------------------------------------------------------------------
// describe("viewRecordSummary")
// ---------------------------------------------------------------------------
describe("viewRecordSummary", () => {
  it("should return text for user_message", () => {
    scenario(
      () => makeRecord("user_message", { content: "Fix the login bug" }),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toBe("Fix the login bug"),
    );
  });

  it("should return text for assistant_message with text blocks", () => {
    scenario(
      () =>
        makeRecord("assistant_message", {
          model: "claude-sonnet-4-20250514",
          content: [{ type: "text", text: "I'll fix that now." }],
        }),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toBe("I'll fix that now."),
    );
  });

  it("should return 'Thinking...' for reasoning", () => {
    scenario(
      () => makeRecord("reasoning", { summary: [], content: "Let me think...", encrypted: false }),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toBe("Thinking..."),
    );
  });

  it("should return tool name + summary for tool_call", () => {
    scenario(
      () =>
        makeRecord("tool_call", {
          call_id: "toolu_1",
          name: "Bash",
          input: { command: "cargo test" },
          raw_input: { command: "cargo test" },
          typed_input: { tool: "bash", command: "cargo test" },
        } as ToolCall),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toBe("Bash: cargo test"),
    );
  });

  it("should return tool name + file_path for Edit", () => {
    scenario(
      () =>
        makeRecord("tool_call", {
          call_id: "toolu_2",
          name: "Edit",
          input: {},
          raw_input: {},
          typed_input: { tool: "edit", file_path: "/src/main.rs", old_string: "a", new_string: "b" },
        } as ToolCall),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toBe("Edit: /src/main.rs"),
    );
  });

  it("should return truncated result for tool_result", () => {
    scenario(
      () =>
        makeRecord("tool_result", {
          call_id: "toolu_1",
          output: "test result: ok. 42 passed; 0 failed",
          is_error: false,
        } as ToolResult),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toContain("test result"),
    );
  });

  it("should return duration for turn_end", () => {
    scenario(
      () => makeRecord("turn_end", { duration_ms: 4500 }),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toBe("Turn completed (4.5s)"),
    );
  });

  it("should return error message for error records", () => {
    scenario(
      () => makeRecord("error", { code: "rate_limit", message: "Too many requests" }),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toBe("Too many requests"),
    );
  });

  it("should return token counts for token_usage", () => {
    scenario(
      () => makeRecord("token_usage", { input_tokens: 1000, output_tokens: 500, scope: "turn" }),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toContain("1000"),
    );
  });

  it("should handle tool_call without typed_input", () => {
    scenario(
      () =>
        makeRecord("tool_call", {
          call_id: "toolu_3",
          name: "mcp__slack__post",
          input: {},
          raw_input: {},
        } as ToolCall),
      (r) => viewRecordSummary(r),
      (result) => expect(result).toBe("mcp__slack__post"),
    );
  });
});
