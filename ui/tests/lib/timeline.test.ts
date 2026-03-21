import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { toTimelineRows } from "@/lib/timeline";
import type { ViewRecord } from "@/types/view-record";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function makeRecord(
  recordType: ViewRecord["record_type"],
  payload: unknown,
  overrides?: Partial<ViewRecord>,
): ViewRecord {
  return {
    id: "evt-1",
    seq: 1,
    session_id: "sess-1",
    timestamp: "2025-01-09T10:00:00Z",
    record_type: recordType,
    payload: payload as ViewRecord["payload"],
    agent_id: null,
    is_sidechain: false,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// describe("toTimelineRows")
// ---------------------------------------------------------------------------
describe("toTimelineRows", () => {
  it("should flatten events from multiple sessions into one sorted list", () => {
    scenario(
      () => [
        makeRecord("user_message", { content: "first" }, {
          id: "a", session_id: "sess-1", timestamp: "2025-01-09T10:00:01Z",
        }),
        makeRecord("user_message", { content: "second" }, {
          id: "b", session_id: "sess-2", timestamp: "2025-01-09T10:00:00Z",
        }),
        makeRecord("tool_call", { call_id: "c1", name: "Read", input: {}, raw_input: {} }, {
          id: "c", session_id: "sess-1", timestamp: "2025-01-09T10:00:02Z",
        }),
      ],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(3);
        // Newest first: 10:00:02 (sess-1), 10:00:01 (sess-1), 10:00:00 (sess-2)
        expect(rows[0]!.sessionId).toBe("sess-1");
        expect(rows[1]!.sessionId).toBe("sess-1");
        expect(rows[2]!.sessionId).toBe("sess-2");
      },
    );
  });

  it("should produce a prompt row from a user_message record", () => {
    scenario(
      () => [makeRecord("user_message", { content: "Fix the login bug" })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("prompt");
        expect(rows[0]!.summary).toBe("Fix the login bug");
      },
    );
  });

  it("should produce a tool row from a tool_call record with tool name and input summary", () => {
    scenario(
      () => [makeRecord("tool_call", {
        call_id: "c1",
        name: "Read",
        input: {},
        raw_input: {},
        typed_input: { tool: "read", file_path: "/src/app.ts" },
      })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("tool");
        expect(rows[0]!.toolName).toBe("Read");
        expect(rows[0]!.summary).toContain("/src/app.ts");
      },
    );
  });

  it("should produce a result row from a tool_result record", () => {
    scenario(
      () => [makeRecord("tool_result", {
        call_id: "c1",
        output: "File contents here...",
        is_error: false,
      })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("result");
        expect(rows[0]!.summary).toContain("File contents");
      },
    );
  });

  it("should produce a response row from an assistant_message record", () => {
    scenario(
      () => [makeRecord("assistant_message", {
        model: "claude-opus-4-6",
        content: [{ type: "text", text: "I found the bug in auth.ts" }],
      })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("response");
        expect(rows[0]!.summary).toContain("I found the bug");
      },
    );
  });

  it("should produce a thinking row from a reasoning record", () => {
    scenario(
      () => [makeRecord("reasoning", {
        summary: ["Analyzing the code structure"],
        encrypted: false,
      })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("thinking");
        expect(rows[0]!.summary).toContain("Analyzing");
      },
    );
  });

  it("should produce an error row from an error record", () => {
    scenario(
      () => [makeRecord("error", {
        code: "TOOL_ERROR",
        message: "Permission denied",
      })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("error");
        expect(rows[0]!.summary).toContain("Permission denied");
      },
    );
  });

  it("should produce a system row from system_event records", () => {
    scenario(
      () => [makeRecord("system_event", {
        subtype: "system.turn.complete",
        duration_ms: 5200,
      })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("system");
      },
    );
  });

  it("should produce a turn row from turn_end records", () => {
    scenario(
      () => [makeRecord("turn_end", { duration_ms: 3000 })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("turn");
      },
    );
  });

  it("should sort rows by timestamp descending (newest first)", () => {
    scenario(
      () => [
        makeRecord("user_message", { content: "third" }, {
          id: "c", timestamp: "2025-01-09T10:00:03Z",
        }),
        makeRecord("user_message", { content: "first" }, {
          id: "a", timestamp: "2025-01-09T10:00:01Z",
        }),
        makeRecord("user_message", { content: "second" }, {
          id: "b", timestamp: "2025-01-09T10:00:02Z",
        }),
      ],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows.map(r => r.summary)).toEqual(["third", "second", "first"]);
      },
    );
  });

  it("should include session_id on each row", () => {
    scenario(
      () => [
        makeRecord("user_message", { content: "hello" }, { session_id: "sess-abc" }),
      ],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows[0]!.sessionId).toBe("sess-abc");
      },
    );
  });

  it("should return empty array for empty input", () => {
    scenario(
      () => [] as ViewRecord[],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(0);
      },
    );
  });

  it("should skip token_usage and session_meta records", () => {
    scenario(
      () => [
        makeRecord("token_usage", { input_tokens: 100, output_tokens: 50, scope: "turn" }),
        makeRecord("session_meta", { cwd: "/project", model: "claude", version: "1.0" }),
        makeRecord("user_message", { content: "hello" }, { id: "keep" }),
      ],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("prompt");
      },
    );
  });

  it("should mark error tool_result rows as error category", () => {
    scenario(
      () => [makeRecord("tool_result", {
        call_id: "c1",
        output: "Command failed with exit code 1",
        is_error: true,
      })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows).toHaveLength(1);
        expect(rows[0]!.category).toBe("error");
      },
    );
  });

  it("should extract text from content blocks in assistant_message", () => {
    scenario(
      () => [makeRecord("assistant_message", {
        model: "claude-opus-4-6",
        content: [
          { type: "text", text: "Here is the fix:" },
          { type: "code_block", text: "const x = 1;", language: "typescript" },
        ],
      })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows[0]!.summary).toContain("Here is the fix");
      },
    );
  });

  it("should truncate very long summaries", () => {
    const longText = "A".repeat(600);
    scenario(
      () => [makeRecord("user_message", { content: longText })],
      (records) => toTimelineRows(records),
      (rows) => {
        expect(rows[0]!.summary.length).toBeLessThanOrEqual(501);
      },
    );
  });
});

describe("toTimelineRows — record type edge cases", () => {
  it("tool_result with is_error=true maps to error category", () => {
    const records = [
      makeRecord("tool_result", { call_id: "c1", output: "command failed", is_error: true }),
    ];
    const rows = toTimelineRows(records);
    expect(rows).toHaveLength(1);
    expect(rows[0]!.category).toBe("error");
    expect(rows[0]!.summary).toContain("command failed");
  });

  it("tool_result with is_error=true and no output shows fallback", () => {
    const records = [
      makeRecord("tool_result", { call_id: "c1", is_error: true }),
    ];
    const rows = toTimelineRows(records);
    expect(rows[0]!.summary).toBe("Tool error");
  });

  it("system_event maps to system category", () => {
    const records = [
      makeRecord("system_event", { subtype: "hook", message: "Hook executed" }),
    ];
    const rows = toTimelineRows(records);
    expect(rows).toHaveLength(1);
    expect(rows[0]!.category).toBe("system");
    expect(rows[0]!.summary).toBe("Hook executed");
  });

  it("system_event falls back to subtype when message is null", () => {
    const records = [
      makeRecord("system_event", { subtype: "compact" }),
    ];
    const rows = toTimelineRows(records);
    expect(rows[0]!.summary).toBe("compact");
  });

  it("turn_end with duration_ms maps to turn category", () => {
    const records = [
      makeRecord("turn_end", { duration_ms: 5500 }),
    ];
    const rows = toTimelineRows(records);
    expect(rows).toHaveLength(1);
    expect(rows[0]!.category).toBe("turn");
    expect(rows[0]!.summary).toBe("Turn complete 5.5s");
  });

  it("turn_end without duration_ms shows no duration", () => {
    const records = [
      makeRecord("turn_end", {}),
    ];
    const rows = toTimelineRows(records);
    expect(rows[0]!.summary).toBe("Turn complete");
  });

  it("turn_end with duration in minutes range", () => {
    const records = [
      makeRecord("turn_end", { duration_ms: 125000 }),
    ];
    const rows = toTimelineRows(records);
    expect(rows[0]!.summary).toBe("Turn complete 2m 5s");
  });

  it("turn_end with exact minutes (no seconds)", () => {
    const records = [
      makeRecord("turn_end", { duration_ms: 180000 }),
    ];
    const rows = toTimelineRows(records);
    expect(rows[0]!.summary).toBe("Turn complete 3m");
  });

  it("turn_end with sub-second duration", () => {
    const records = [
      makeRecord("turn_end", { duration_ms: 450 }),
    ];
    const rows = toTimelineRows(records);
    expect(rows[0]!.summary).toBe("Turn complete 450ms");
  });
});

describe("toTimelineRows — file hint propagation", () => {
  it("attaches fileHint from preceding tool_call to tool_result", () => {
    const records = [
      makeRecord("tool_call", {
        call_id: "c1",
        name: "Read",
        input: {},
        raw_input: {},
        typed_input: { tool: "read", file_path: "/src/main.ts" },
      }, { id: "tc1", timestamp: "2025-01-09T10:00:00Z" }),
      makeRecord("tool_result", {
        call_id: "c1",
        output: "file contents here",
        is_error: false,
      }, { id: "tr1", timestamp: "2025-01-09T10:00:01Z" }),
    ];
    const rows = toTimelineRows(records);
    const resultRow = rows.find(r => r.category === "result");
    expect(resultRow).toBeDefined();
    expect(resultRow!.fileHint).toBe("/src/main.ts");
  });

  it("clears fileHint after consuming tool_result", () => {
    const records = [
      makeRecord("tool_call", {
        call_id: "c1", name: "Read", input: {}, raw_input: {},
        typed_input: { tool: "read", file_path: "/src/a.ts" },
      }, { id: "tc1", timestamp: "2025-01-09T10:00:00Z" }),
      makeRecord("tool_result", {
        call_id: "c1", output: "a content", is_error: false,
      }, { id: "tr1", timestamp: "2025-01-09T10:00:01Z" }),
      makeRecord("tool_result", {
        call_id: "c2", output: "orphan result", is_error: false,
      }, { id: "tr2", timestamp: "2025-01-09T10:00:02Z" }),
    ];
    const rows = toTimelineRows(records);
    // Rows sorted descending: tr2 (10:00:02), tr1 (10:00:01), tc1 (10:00:00)
    const firstResult = rows.find(r => r.id === "tr1");
    expect(firstResult!.fileHint).toBe("/src/a.ts");
    // Second result should NOT have file hint (cleared after first)
    const orphanResult = rows.find(r => r.id === "tr2");
    expect(orphanResult!.fileHint).toBeUndefined();
  });

  it("tool_call without typed_input clears fileHint", () => {
    const records = [
      makeRecord("tool_call", {
        call_id: "c1", name: "Bash", input: {}, raw_input: {},
      }, { id: "tc1", timestamp: "2025-01-09T10:00:00Z" }),
      makeRecord("tool_result", {
        call_id: "c1", output: "output", is_error: false,
      }, { id: "tr1", timestamp: "2025-01-09T10:00:01Z" }),
    ];
    const rows = toTimelineRows(records);
    const resultRow = rows.find(r => r.category === "result");
    expect(resultRow!.fileHint).toBeUndefined();
  });

  it("tool_call with typed_input but no file_path has no fileHint", () => {
    const records = [
      makeRecord("tool_call", {
        call_id: "c1", name: "Bash", input: {}, raw_input: {},
        typed_input: { tool: "bash", command: "ls" },
      }, { id: "tc1", timestamp: "2025-01-09T10:00:00Z" }),
      makeRecord("tool_result", {
        call_id: "c1", output: "files", is_error: false,
      }, { id: "tr1", timestamp: "2025-01-09T10:00:01Z" }),
    ];
    const rows = toTimelineRows(records);
    const resultRow = rows.find(r => r.category === "result");
    expect(resultRow!.fileHint).toBeUndefined();
  });
});
