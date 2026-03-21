import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  viewRecordLabel,
  viewRecordSummary,
  viewRecordColor,
  isGitBashRecord,
} from "@/lib/view-record-transforms";
import type { ViewRecord, RecordType } from "@/types/view-record";

// ---------------------------------------------------------------------------
// Helper: minimal ViewRecord factory
// ---------------------------------------------------------------------------

function makeRecord(
  overrides: Partial<ViewRecord> & { record_type: RecordType; payload: any },
): ViewRecord {
  return {
    id: "test-id",
    seq: 1,
    session_id: "test-session",
    timestamp: "2026-01-01T00:00:00Z",
    agent_id: null,
    is_sidechain: false,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// viewRecordLabel — all 13 record types
// ---------------------------------------------------------------------------

describe("viewRecordLabel", () => {
  it.each([
    ["user_message", "Prompt"],
    ["assistant_message", "Response"],
    ["reasoning", "Thinking"],
    ["tool_call", "Tool Use"],
    ["tool_result", "Result"],
    ["turn_end", "Complete"],
    ["token_usage", "Tokens"],
    ["system_event", "System"],
    ["error", "Error"],
    ["session_meta", "Start"],
    ["turn_start", "Turn"],
    ["context_compaction", "Compact"],
    ["file_snapshot", "Snapshot"],
  ] as [RecordType, string][])("viewRecordLabel(%j) => %j", (type, label) => {
    expect(viewRecordLabel(type)).toBe(label);
  });
});

// ---------------------------------------------------------------------------
// viewRecordSummary
// ---------------------------------------------------------------------------

describe("viewRecordSummary", () => {
  it("user_message with string content", () => {
    scenario(
      () =>
        makeRecord({
          record_type: "user_message",
          payload: { content: "Hello, world!" },
        }),
      (record) => viewRecordSummary(record),
      (summary) => expect(summary).toBe("Hello, world!"),
    );
  });

  it("user_message with content blocks", () => {
    const record = makeRecord({
      record_type: "user_message",
      payload: {
        content: [
          { type: "text", text: "Block one" },
          { type: "text", text: "Block two" },
        ],
      },
    });
    expect(viewRecordSummary(record)).toBe("Block one\nBlock two");
  });

  it("assistant_message extracts text blocks only", () => {
    const record = makeRecord({
      record_type: "assistant_message",
      payload: {
        model: "claude-3",
        content: [
          { type: "text", text: "Response text" },
          { type: "code_block", language: "ts", text: "const x = 1;" },
        ],
      },
    });
    // extractText filters for type === "text" — code_block is excluded
    expect(viewRecordSummary(record)).toBe("Response text");
  });

  it("assistant_message joins multiple text blocks", () => {
    const record = makeRecord({
      record_type: "assistant_message",
      payload: {
        model: "claude-3",
        content: [
          { type: "text", text: "Part one" },
          { type: "text", text: "Part two" },
        ],
      },
    });
    expect(viewRecordSummary(record)).toBe("Part one\nPart two");
  });

  it("reasoning returns Thinking...", () => {
    const record = makeRecord({
      record_type: "reasoning",
      payload: { summary: [], encrypted: false },
    });
    expect(viewRecordSummary(record)).toBe("Thinking...");
  });

  it("tool_call with name and typed_input", () => {
    scenario(
      () =>
        makeRecord({
          record_type: "tool_call",
          payload: {
            name: "Bash",
            call_id: "c1",
            input: {},
            raw_input: {},
            typed_input: { tool: "bash", command: "cargo test" },
          },
        }),
      (record) => viewRecordSummary(record),
      (summary) => expect(summary).toBe("Bash: cargo test"),
    );
  });

  it("tool_call with name only (no typed_input)", () => {
    const record = makeRecord({
      record_type: "tool_call",
      payload: {
        name: "Read",
        call_id: "c2",
        input: {},
        raw_input: {},
      },
    });
    expect(viewRecordSummary(record)).toBe("Read");
  });

  it("tool_call with read typed_input returns file_path", () => {
    const record = makeRecord({
      record_type: "tool_call",
      payload: {
        name: "Read",
        call_id: "c3",
        input: {},
        raw_input: {},
        typed_input: { tool: "read", file_path: "/src/main.rs" },
      },
    });
    expect(viewRecordSummary(record)).toBe("Read: /src/main.rs");
  });

  it("tool_result with output", () => {
    const record = makeRecord({
      record_type: "tool_result",
      payload: {
        call_id: "c1",
        output: "All tests passed",
        is_error: false,
      },
    });
    expect(viewRecordSummary(record)).toBe("All tests passed");
  });

  it("tool_result truncates long output at 120 chars", () => {
    const longOutput = "x".repeat(200);
    const record = makeRecord({
      record_type: "tool_result",
      payload: {
        call_id: "c1",
        output: longOutput,
        is_error: false,
      },
    });
    const summary = viewRecordSummary(record);
    expect(summary).toHaveLength(121); // 120 chars + ellipsis
    expect(summary.endsWith("\u2026")).toBe(true);
  });

  it("tool_result with no output", () => {
    const record = makeRecord({
      record_type: "tool_result",
      payload: {
        call_id: "c1",
        is_error: false,
      },
    });
    expect(viewRecordSummary(record)).toBe("");
  });

  it("turn_end with duration_ms", () => {
    scenario(
      () =>
        makeRecord({
          record_type: "turn_end",
          payload: { duration_ms: 3500 },
        }),
      (record) => viewRecordSummary(record),
      (summary) => expect(summary).toBe("Turn completed (3.5s)"),
    );
  });

  it("turn_end without duration_ms", () => {
    const record = makeRecord({
      record_type: "turn_end",
      payload: {},
    });
    expect(viewRecordSummary(record)).toBe("Turn completed");
  });

  it("error returns message", () => {
    const record = makeRecord({
      record_type: "error",
      payload: { code: "E001", message: "Something broke" },
    });
    expect(viewRecordSummary(record)).toBe("Something broke");
  });

  it("token_usage with both tokens", () => {
    const record = makeRecord({
      record_type: "token_usage",
      payload: { input_tokens: 1500, output_tokens: 800, scope: "turn" },
    });
    expect(viewRecordSummary(record)).toBe("1500 in / 800 out");
  });

  it("token_usage with only input_tokens", () => {
    const record = makeRecord({
      record_type: "token_usage",
      payload: { input_tokens: 1500, scope: "turn" },
    });
    expect(viewRecordSummary(record)).toBe("1500 in");
  });

  it("token_usage with no tokens", () => {
    const record = makeRecord({
      record_type: "token_usage",
      payload: { scope: "turn" },
    });
    expect(viewRecordSummary(record)).toBe("Token usage");
  });

  it("session_meta returns empty (default branch)", () => {
    const record = makeRecord({
      record_type: "session_meta",
      payload: { cwd: "/project", model: "claude-3", version: "1.0" },
    });
    expect(viewRecordSummary(record)).toBe("");
  });

  it("turn_start returns empty (default branch)", () => {
    const record = makeRecord({
      record_type: "turn_start",
      payload: {},
    });
    expect(viewRecordSummary(record)).toBe("");
  });
});

// ---------------------------------------------------------------------------
// viewRecordColor — all 13 record types
// ---------------------------------------------------------------------------

describe("viewRecordColor", () => {
  it.each([
    ["user_message", "#7aa2f7"],
    ["assistant_message", "#bb9af7"],
    ["reasoning", "#9ece6a"],
    ["tool_call", "#2ac3de"],
    ["tool_result", "#2ac3de"],
    ["turn_end", "#565f89"],
    ["turn_start", "#565f89"],
    ["token_usage", "#565f89"],
    ["system_event", "#565f89"],
    ["error", "#f7768e"],
    ["session_meta", "#9ece6a"],
    ["context_compaction", "#565f89"],
    ["file_snapshot", "#565f89"],
  ] as [RecordType, string][])(
    "viewRecordColor(%s) => %s",
    (recordType, expectedColor) => {
      const record = makeRecord({
        record_type: recordType,
        payload: recordType === "error"
          ? { code: "E", message: "err" }
          : recordType === "tool_call"
            ? { name: "Read", call_id: "c1", input: {}, raw_input: {} }
            : {},
      });
      expect(viewRecordColor(record)).toBe(expectedColor);
    },
  );

  it("overrides with git risk color for safe git bash", () => {
    scenario(
      () =>
        makeRecord({
          record_type: "tool_call",
          payload: {
            name: "Bash",
            call_id: "c1",
            input: {},
            raw_input: {},
            typed_input: { tool: "bash", command: "git status" },
          },
        }),
      (record) => viewRecordColor(record),
      (color) => expect(color).toBe("#565f89"), // safe
    );
  });

  it("overrides with git risk color for destructive git command", () => {
    scenario(
      () =>
        makeRecord({
          record_type: "tool_call",
          payload: {
            name: "Bash",
            call_id: "c1",
            input: {},
            raw_input: {},
            typed_input: { tool: "bash", command: "git push --force origin main" },
          },
        }),
      (record) => viewRecordColor(record),
      (color) => expect(color).toBe("#f7768e"), // destructive
    );
  });

  it("overrides with git risk color for commit git command", () => {
    const record = makeRecord({
      record_type: "tool_call",
      payload: {
        name: "Bash",
        call_id: "c1",
        input: {},
        raw_input: {},
        typed_input: { tool: "bash", command: "git commit -m 'fix'" },
      },
    });
    expect(viewRecordColor(record)).toBe("#9ece6a"); // commit
  });
});

// ---------------------------------------------------------------------------
// isGitBashRecord
// ---------------------------------------------------------------------------

describe("isGitBashRecord", () => {
  it.each([
    [
      "tool_call + Bash + git command",
      makeRecord({
        record_type: "tool_call",
        payload: {
          name: "Bash",
          call_id: "c1",
          input: {},
          raw_input: {},
          typed_input: { tool: "bash", command: "git status" },
        },
      }),
      true,
    ],
    [
      "tool_call + Bash + non-git command",
      makeRecord({
        record_type: "tool_call",
        payload: {
          name: "Bash",
          call_id: "c1",
          input: {},
          raw_input: {},
          typed_input: { tool: "bash", command: "cargo test" },
        },
      }),
      false,
    ],
    [
      "tool_call + Read + git-like args",
      makeRecord({
        record_type: "tool_call",
        payload: {
          name: "Read",
          call_id: "c1",
          input: {},
          raw_input: {},
          typed_input: { tool: "read", file_path: "git-status.txt" },
        },
      }),
      false,
    ],
    [
      "user_message (not tool_call)",
      makeRecord({
        record_type: "user_message",
        payload: { content: "git status" },
      }),
      false,
    ],
    [
      "tool_call + Bash but no typed_input",
      makeRecord({
        record_type: "tool_call",
        payload: {
          name: "Bash",
          call_id: "c1",
          input: {},
          raw_input: {},
        },
      }),
      false,
    ],
    [
      "tool_call + Bash + git with cd prefix",
      makeRecord({
        record_type: "tool_call",
        payload: {
          name: "Bash",
          call_id: "c1",
          input: {},
          raw_input: {},
          typed_input: { tool: "bash", command: "cd /project && git log" },
        },
      }),
      true,
    ],
  ] as [string, ViewRecord, boolean][])(
    "%s => %s",
    (_label, record, expected) => {
      expect(isGitBashRecord(record)).toBe(expected);
    },
  );
});
