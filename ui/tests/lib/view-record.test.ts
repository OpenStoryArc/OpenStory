import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  isToolCall,
  isToolRoundtrip,
  toolInputSummary,
  type ViewRecord,
  type ToolInput,
  type ConversationEntry,
  type ToolRoundtripEntry,
  type ToolCall,
} from "@/types/view-record";

// ---------------------------------------------------------------------------
// describe("toolInputSummary")
// ---------------------------------------------------------------------------
describe("toolInputSummary", () => {
  // Boundary table: tool → expected summary field
  const cases: [string, ToolInput, string][] = [
    ["Read", { tool: "read", file_path: "/src/main.rs" }, "/src/main.rs"],
    ["Edit", { tool: "edit", file_path: "/src/lib.rs", old_string: "a", new_string: "b" }, "/src/lib.rs"],
    ["Write", { tool: "write", file_path: "/new.rs", content: "fn main() {}" }, "/new.rs"],
    ["Bash", { tool: "bash", command: "cargo test" }, "cargo test"],
    ["Grep", { tool: "grep", pattern: "fn main" }, "fn main"],
    ["Glob", { tool: "glob", pattern: "**/*.rs" }, "**/*.rs"],
    ["WebSearch", { tool: "web_search", query: "rust serde" }, "rust serde"],
    ["WebFetch", { tool: "web_fetch", url: "https://example.com" }, "https://example.com"],
    ["Agent with description", { tool: "agent", prompt: "long prompt", description: "search TODOs" }, "search TODOs"],
    ["Agent without description", { tool: "agent", prompt: "Find all TODO comments" }, "Find all TODO comments"],
    ["Skill", { tool: "skill", skill: "commit" }, "commit"],
    ["NotebookEdit", { tool: "notebook_edit", notebook_path: "/nb.ipynb", new_source: "x" }, "/nb.ipynb"],
    ["Unknown", { tool: "unknown", name: "mcp__slack", raw: {} }, ""],
    ["TaskList (parameterless)", { tool: "task_list" }, ""],
    ["EnterPlanMode", { tool: "enter_plan_mode" }, ""],
  ];

  it.each(cases)("should return correct summary for %s", (_label, input, expected) => {
    scenario(
      () => input,
      (input) => toolInputSummary(input),
      (result) => expect(result).toBe(expected),
    );
  });

  it("should return empty string for undefined input", () => {
    expect(toolInputSummary(undefined)).toBe("");
  });
});

// ---------------------------------------------------------------------------
// describe("isToolCall type guard")
// ---------------------------------------------------------------------------
describe("isToolCall", () => {
  it("should return true for tool_call records", () => {
    const record: ViewRecord = {
      id: "e1",
      seq: 1,
      session_id: "s1",
      timestamp: "2025-01-09T10:00:00Z",
      record_type: "tool_call",
      agent_id: null,
      is_sidechain: false,
      payload: {
        call_id: "toolu_1",
        name: "Bash",
        input: { command: "ls" },
        raw_input: { command: "ls" },
        typed_input: { tool: "bash", command: "ls" },
      } as ToolCall,
    };
    expect(isToolCall(record)).toBe(true);
  });

  it("should return false for non-tool records", () => {
    const record: ViewRecord = {
      id: "e2",
      seq: 2,
      session_id: "s1",
      timestamp: "2025-01-09T10:00:00Z",
      record_type: "user_message",
      agent_id: null,
      is_sidechain: false,
      payload: { content: "hello" },
    };
    expect(isToolCall(record)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// describe("isToolRoundtrip type guard")
// ---------------------------------------------------------------------------
describe("isToolRoundtrip", () => {
  it("should return true for tool_roundtrip entries", () => {
    const entry: ToolRoundtripEntry = {
      entry_type: "tool_roundtrip",
      call: {
        id: "e1",
        seq: 1,
        session_id: "s1",
        timestamp: "2025-01-09T10:00:00Z",
        record_type: "tool_call",
        agent_id: null,
        is_sidechain: false,
        payload: {
          call_id: "toolu_1",
          name: "Bash",
          input: {},
          raw_input: {},
        },
      },
      result: null,
    };
    expect(isToolRoundtrip(entry)).toBe(true);
  });

  it("should return false for user_message entries", () => {
    const entry: ConversationEntry = {
      entry_type: "user_message",
      id: "e2",
      seq: 2,
      session_id: "s1",
      timestamp: "2025-01-09T10:00:00Z",
      record_type: "user_message",
      payload: { content: "hello" },
    };
    expect(isToolRoundtrip(entry)).toBe(false);
  });
});
