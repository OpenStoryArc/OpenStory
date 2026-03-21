import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { searchRecords, highlightMatch, type HighlightSegment } from "@/lib/explore-search";
import type { WireRecord } from "@/types/wire-record";

function makeRecord(overrides: Partial<WireRecord>): WireRecord {
  return {
    id: "test-id",
    seq: 1,
    session_id: "s1",
    timestamp: "2026-01-01T00:00:00Z",
    record_type: "assistant_message",
    payload: { text: "hello world" },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 100,
    ...overrides,
  } as WireRecord;
}

// ── searchRecords ──────────────────────

describe("searchRecords", () => {
  it("empty query returns all records", () => {
    const records = [makeRecord({ id: "1" }), makeRecord({ id: "2" })];
    scenario(
      () => ({ records, query: "" }),
      ({ records, query }) => searchRecords(records, query),
      (result) => expect(result).toHaveLength(2),
    );
  });

  it("matches text content", () => {
    const records = [
      makeRecord({ id: "1", payload: { text: "Fix the authentication bug" } }),
      makeRecord({ id: "2", payload: { text: "Add tests" } }),
    ];
    scenario(
      () => ({ records, query: "auth" }),
      ({ records, query }) => searchRecords(records, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.id).toBe("1");
      },
    );
  });

  it("matches tool name", () => {
    const records = [
      makeRecord({ id: "1", record_type: "tool_call", payload: { name: "Read", raw_input: { file_path: "a.ts" } } }),
      makeRecord({ id: "2", record_type: "tool_call", payload: { name: "Bash", raw_input: { command: "ls" } } }),
    ];
    scenario(
      () => ({ records, query: "read" }),
      ({ records, query }) => searchRecords(records, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.id).toBe("1");
      },
    );
  });

  it("matches file path in tool input", () => {
    const records = [
      makeRecord({ id: "1", record_type: "tool_call", payload: { name: "Read", raw_input: { file_path: "src/config.rs" } } }),
      makeRecord({ id: "2", record_type: "tool_call", payload: { name: "Read", raw_input: { file_path: "src/main.rs" } } }),
    ];
    scenario(
      () => ({ records, query: "config" }),
      ({ records, query }) => searchRecords(records, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.id).toBe("1");
      },
    );
  });

  it("matches command in bash tool", () => {
    const records = [
      makeRecord({ id: "1", record_type: "tool_call", payload: { name: "Bash", raw_input: { command: "cargo test" } } }),
      makeRecord({ id: "2", record_type: "tool_call", payload: { name: "Bash", raw_input: { command: "npm install" } } }),
    ];
    scenario(
      () => ({ records, query: "cargo" }),
      ({ records, query }) => searchRecords(records, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.id).toBe("1");
      },
    );
  });

  it("matches tool result output", () => {
    const records = [
      makeRecord({ id: "1", record_type: "tool_result", payload: { output: "test result: ok. 15 passed" } }),
      makeRecord({ id: "2", record_type: "tool_result", payload: { output: "file not found" } }),
    ];
    scenario(
      () => ({ records, query: "passed" }),
      ({ records, query }) => searchRecords(records, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.id).toBe("1");
      },
    );
  });

  it("case insensitive", () => {
    const records = [
      makeRecord({ id: "1", payload: { text: "HELLO WORLD" } }),
    ];
    scenario(
      () => ({ records, query: "hello" }),
      ({ records, query }) => searchRecords(records, query),
      (result) => expect(result).toHaveLength(1),
    );
  });

  it("no match returns empty", () => {
    const records = [makeRecord({ id: "1", payload: { text: "hello" } })];
    scenario(
      () => ({ records, query: "zzz" }),
      ({ records, query }) => searchRecords(records, query),
      (result) => expect(result).toHaveLength(0),
    );
  });
});

// ── highlightMatch — boundary table ──────────────────────

const HIGHLIGHT_TABLE: [string, string, string, HighlightSegment[]][] = [
  [
    "no match",
    "hello world",
    "xyz",
    [{ text: "hello world", isMatch: false }],
  ],
  [
    "empty query",
    "hello world",
    "",
    [{ text: "hello world", isMatch: false }],
  ],
  [
    "single match at start",
    "hello world",
    "hello",
    [{ text: "hello", isMatch: true }, { text: " world", isMatch: false }],
  ],
  [
    "single match at end",
    "hello world",
    "world",
    [{ text: "hello ", isMatch: false }, { text: "world", isMatch: true }],
  ],
  [
    "match in middle",
    "the quick brown fox",
    "quick",
    [{ text: "the ", isMatch: false }, { text: "quick", isMatch: true }, { text: " brown fox", isMatch: false }],
  ],
  [
    "multiple matches",
    "foo bar foo baz foo",
    "foo",
    [
      { text: "foo", isMatch: true },
      { text: " bar ", isMatch: false },
      { text: "foo", isMatch: true },
      { text: " baz ", isMatch: false },
      { text: "foo", isMatch: true },
    ],
  ],
  [
    "case insensitive match",
    "Hello World",
    "hello",
    [{ text: "Hello", isMatch: true }, { text: " World", isMatch: false }],
  ],
  [
    "entire string matches",
    "test",
    "test",
    [{ text: "test", isMatch: true }],
  ],
];

describe("highlightMatch — boundary table", () => {
  it.each(HIGHLIGHT_TABLE)(
    "%s",
    (_desc, text, query, expected) => {
      scenario(
        () => ({ text, query }),
        ({ text, query }) => highlightMatch(text, query),
        (result) => expect(result).toEqual(expected),
      );
    },
  );
});
