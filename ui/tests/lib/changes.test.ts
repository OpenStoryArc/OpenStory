import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  extractFileChanges,
  pairReadWithWrite,
  hunkStats,
  type FileChange,
  type FileHunk,
} from "@/lib/changes";
import type { ViewRecord, ToolCall, ToolResult } from "@/types/view-record";

let seqCounter = 0;

/** Helper to build a minimal ViewRecord for testing */
function makeRecord(overrides: Partial<ViewRecord> = {}): ViewRecord {
  seqCounter++;
  return {
    id: overrides.id ?? `rec-${seqCounter}`,
    seq: overrides.seq ?? seqCounter,
    session_id: overrides.session_id ?? "test-session",
    timestamp: overrides.timestamp ?? "2025-01-08T10:00:00Z",
    record_type: overrides.record_type ?? "tool_call",
    payload: overrides.payload ?? { call_id: "c1", name: "Unknown", input: {}, raw_input: {} },
    agent_id: overrides.agent_id ?? null,
    is_sidechain: overrides.is_sidechain ?? false,
  };
}

function editRecord(
  id: string,
  filePath: string,
  oldString: string,
  newString: string,
  timestamp = "2025-01-08T10:00:00Z",
  replaceAll?: boolean,
): ViewRecord {
  return makeRecord({
    id,
    timestamp,
    record_type: "tool_call",
    payload: {
      call_id: `call-${id}`,
      name: "Edit",
      input: {},
      raw_input: {},
      typed_input: {
        tool: "edit",
        file_path: filePath,
        old_string: oldString,
        new_string: newString,
        ...(replaceAll !== undefined ? { replace_all: replaceAll } : {}),
      },
    } as ToolCall,
  });
}

function writeRecord(
  id: string,
  filePath: string,
  content: string,
  timestamp = "2025-01-08T10:00:00Z",
): ViewRecord {
  return makeRecord({
    id,
    timestamp,
    record_type: "tool_call",
    payload: {
      call_id: `call-${id}`,
      name: "Write",
      input: {},
      raw_input: {},
      typed_input: {
        tool: "write",
        file_path: filePath,
        content,
      },
    } as ToolCall,
  });
}

function readCallRecord(
  id: string,
  filePath: string,
  timestamp = "2025-01-08T09:59:00Z",
): ViewRecord {
  return makeRecord({
    id,
    timestamp,
    record_type: "tool_call",
    payload: {
      call_id: `call-${id}`,
      name: "Read",
      input: {},
      raw_input: {},
      typed_input: {
        tool: "read",
        file_path: filePath,
      },
    } as ToolCall,
  });
}

function readResultRecord(
  id: string,
  callId: string,
  result: string,
  timestamp = "2025-01-08T09:59:01Z",
): ViewRecord {
  return makeRecord({
    id,
    timestamp,
    record_type: "tool_result",
    payload: {
      call_id: callId,
      output: result,
      is_error: false,
    } as ToolResult,
  });
}

describe("extractFileChanges", () => {
  it("should extract a single Edit record into one file with one hunk", () => {
    scenario(
      () => [editRecord("e1", "/src/app.ts", "old code", "new code")],
      (records) => extractFileChanges(records),
      (changes) => {
        expect(changes).toHaveLength(1);
        expect(changes[0]!.filePath).toBe("/src/app.ts");
        expect(changes[0]!.fileName).toBe("app.ts");
        expect(changes[0]!.hunks).toHaveLength(1);
        expect(changes[0]!.hunks[0]!.tool).toBe("Edit");
        expect(changes[0]!.hunks[0]!.oldText).toBe("old code");
        expect(changes[0]!.hunks[0]!.newText).toBe("new code");
      },
    );
  });

  it("should group multiple Edits to the same file into one FileChange with multiple hunks", () => {
    scenario(
      () => [
        editRecord("e1", "/src/app.ts", "line1", "LINE1", "2025-01-08T10:00:00Z"),
        editRecord("e2", "/src/app.ts", "line2", "LINE2", "2025-01-08T10:01:00Z"),
      ],
      (records) => extractFileChanges(records),
      (changes) => {
        expect(changes).toHaveLength(1);
        expect(changes[0]!.hunks).toHaveLength(2);
        expect(changes[0]!.lastChanged).toBe("2025-01-08T10:01:00Z");
      },
    );
  });

  it("should pair Write with preceding Read to get oldText", () => {
    scenario(
      () => [
        readCallRecord("r1", "/src/config.ts"),
        readResultRecord("r1-result", "call-r1", "old content"),
        writeRecord("w1", "/src/config.ts", "new content"),
      ],
      (records) => extractFileChanges(records),
      (changes) => {
        expect(changes).toHaveLength(1);
        expect(changes[0]!.hunks[0]!.tool).toBe("Write");
        expect(changes[0]!.hunks[0]!.oldText).toBe("old content");
        expect(changes[0]!.hunks[0]!.newText).toBe("new content");
      },
    );
  });

  it("should set oldText to null for Write without preceding Read", () => {
    scenario(
      () => [writeRecord("w1", "/src/new-file.ts", "brand new content")],
      (records) => extractFileChanges(records),
      (changes) => {
        expect(changes).toHaveLength(1);
        expect(changes[0]!.hunks[0]!.oldText).toBeNull();
        expect(changes[0]!.hunks[0]!.newText).toBe("brand new content");
      },
    );
  });

  it("should handle mixed Edit and Write to different files", () => {
    scenario(
      () => [
        editRecord("e1", "/src/a.ts", "old", "new", "2025-01-08T10:00:00Z"),
        writeRecord("w1", "/src/b.ts", "content", "2025-01-08T10:01:00Z"),
      ],
      (records) => extractFileChanges(records),
      (changes) => {
        expect(changes).toHaveLength(2);
        // Sorted by lastChanged descending — b.ts is newer
        expect(changes[0]!.filePath).toBe("/src/b.ts");
        expect(changes[1]!.filePath).toBe("/src/a.ts");
      },
    );
  });

  it("should return empty array when no file operations exist", () => {
    scenario(
      () => [
        makeRecord({
          id: "e1",
          record_type: "assistant_message",
          payload: { model: "test", content: [{ type: "text", text: "hello" }] },
        }),
      ],
      (records) => extractFileChanges(records),
      (changes) => {
        expect(changes).toHaveLength(0);
      },
    );
  });

  it("should preserve replace_all flag on Edit hunks", () => {
    scenario(
      () => [editRecord("e1", "/src/app.ts", "foo", "bar", undefined, true)],
      (records) => extractFileChanges(records),
      (changes) => {
        expect(changes[0]!.hunks[0]!.replaceAll).toBe(true);
      },
    );
  });

  it("should only process tool_call records", () => {
    scenario(
      () => [
        makeRecord({
          id: "e1",
          record_type: "assistant_message",
          payload: { model: "test", content: [{ type: "text", text: "hello" }] },
        }),
      ],
      (records) => extractFileChanges(records),
      (changes) => {
        expect(changes).toHaveLength(0);
      },
    );
  });
});

describe("pairReadWithWrite", () => {
  it("should find the nearest Read result for the same path before the Write", () => {
    const records = [
      readCallRecord("r1", "/src/config.ts"),
      readResultRecord("r1-result", "call-r1", "file content"),
      makeRecord({ id: "e2", record_type: "assistant_message", payload: { model: "test", content: [] } }),
      writeRecord("w1", "/src/config.ts", "new content"),
    ];
    const result = pairReadWithWrite(records, 3);
    expect(result).toBe("file content");
  });

  it("should return null when no Read precedes the Write", () => {
    const records = [writeRecord("w1", "/src/config.ts", "content")];
    const result = pairReadWithWrite(records, 0);
    expect(result).toBeNull();
  });

  it("should not match Read for a different path", () => {
    const records = [
      readCallRecord("r1", "/src/other.ts"),
      readResultRecord("r1-result", "call-r1", "other content"),
      writeRecord("w1", "/src/config.ts", "content"),
    ];
    const result = pairReadWithWrite(records, 2);
    expect(result).toBeNull();
  });
});

describe("hunkStats", () => {
  it("should count files and total edits", () => {
    scenario(
      () =>
        [
          {
            filePath: "/a.ts",
            fileName: "a.ts",
            hunks: [{} as FileHunk, {} as FileHunk],
            lastChanged: "",
          },
          {
            filePath: "/b.ts",
            fileName: "b.ts",
            hunks: [{} as FileHunk],
            lastChanged: "",
          },
        ] as FileChange[],
      (changes) => hunkStats(changes),
      (stats) => {
        expect(stats.filesChanged).toBe(2);
        expect(stats.totalEdits).toBe(3);
      },
    );
  });

  it("should return zeros for empty input", () => {
    const stats = hunkStats([]);
    expect(stats.filesChanged).toBe(0);
    expect(stats.totalEdits).toBe(0);
  });
});

describe("pairReadWithWrite — branch coverage", () => {
  it("returns null when record at writeIndex is not a tool_call", () => {
    const records: ViewRecord[] = [
      makeRecord({ id: "r1", record_type: "tool_result", payload: { call_id: "c1", output: "ok", is_error: false } as ToolResult }),
    ];
    expect(pairReadWithWrite(records, 0)).toBeNull();
  });

  it("returns null when tool_call typed_input is not 'write'", () => {
    const records: ViewRecord[] = [
      makeRecord({
        id: "e1",
        record_type: "tool_call",
        payload: {
          call_id: "c1",
          name: "Edit",
          input: {},
          raw_input: {},
          typed_input: { tool: "edit", file_path: "/a.ts", old_string: "x", new_string: "y" },
        } as ToolCall,
      }),
    ];
    expect(pairReadWithWrite(records, 0)).toBeNull();
  });

  it("returns null when preceding tool_result has no preceding tool_call", () => {
    const records: ViewRecord[] = [
      makeRecord({ id: "r1", record_type: "tool_result", payload: { call_id: "c1", output: "content", is_error: false } as ToolResult }),
      writeRecord("w1", "/src/file.ts", "new content"),
    ];
    expect(pairReadWithWrite(records, 1)).toBeNull();
  });

  it("returns null when preceding tool_call is not a Read", () => {
    const records: ViewRecord[] = [
      makeRecord({
        id: "e1",
        record_type: "tool_call",
        payload: {
          call_id: "c1", name: "Grep", input: {}, raw_input: {},
          typed_input: { tool: "grep", pattern: "test" },
        } as ToolCall,
      }),
      makeRecord({ id: "r1", record_type: "tool_result", payload: { call_id: "c1", output: "content", is_error: false } as ToolResult }),
      writeRecord("w1", "/src/file.ts", "new content"),
    ];
    expect(pairReadWithWrite(records, 2)).toBeNull();
  });

  it("returns null when tool_call typed_input is undefined", () => {
    const records: ViewRecord[] = [
      makeRecord({
        id: "e1",
        record_type: "tool_call",
        payload: { call_id: "c1", name: "Write", input: {}, raw_input: {} } as ToolCall,
      }),
    ];
    expect(pairReadWithWrite(records, 0)).toBeNull();
  });

  it("returns null when Read result output is undefined", () => {
    const records: ViewRecord[] = [
      readCallRecord("r1", "/src/file.ts"),
      makeRecord({ id: "r1-result", record_type: "tool_result", payload: { call_id: "call-r1", is_error: false } as ToolResult }),
      writeRecord("w1", "/src/file.ts", "new content"),
    ];
    const result = pairReadWithWrite(records, 2);
    expect(result).toBeNull();
  });
});

describe("extractFileChanges — branch coverage", () => {
  it("skips tool_call records with no typed_input", () => {
    const records: ViewRecord[] = [
      makeRecord({
        id: "e1",
        record_type: "tool_call",
        payload: { call_id: "c1", name: "Edit", input: {}, raw_input: {} } as ToolCall,
      }),
    ];
    expect(extractFileChanges(records)).toHaveLength(0);
  });

  it("skips non-edit/write typed_input tools", () => {
    const records: ViewRecord[] = [
      makeRecord({
        id: "e1",
        record_type: "tool_call",
        payload: {
          call_id: "c1", name: "Read", input: {}, raw_input: {},
          typed_input: { tool: "read", file_path: "/a.ts" },
        } as ToolCall,
      }),
    ];
    expect(extractFileChanges(records)).toHaveLength(0);
  });

  it("omits replaceAll when it's false/undefined", () => {
    const records = [editRecord("e1", "/a.ts", "old", "new", undefined, false)];
    const changes = extractFileChanges(records);
    expect(changes[0]!.hunks[0]!.replaceAll).toBeUndefined();
  });
});
