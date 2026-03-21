//! Spec: Client-side timeline filter predicates — 9 consolidated filters.
//!
//! Each filter answers one question about agent behavior.
//! The boundary table below IS the spec.

import { describe, it, expect } from "vitest";

import type { WireRecord } from "@/types/wire-record";
import type { ToolInput } from "@/types/view-record";
import { TIMELINE_FILTERS, FILTER_GROUPS } from "@/lib/timeline-filters";

// ── Record factories ──

function makeRecord(overrides: Partial<WireRecord> & { record_type: string }): WireRecord {
  return {
    id: "r-1",
    seq: 1,
    session_id: "s1",
    timestamp: "2025-01-01T00:00:00Z",
    payload: {},
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 10,
    ...overrides,
  };
}

function makeToolCall(tool: string, command?: string): WireRecord {
  return makeRecord({
    record_type: "tool_call",
    payload: {
      call_id: "call-1",
      name: tool,
      input: command ? { command } : {},
      raw_input: command ? { command } : {},
      typed_input: (command
        ? { tool: tool.toLowerCase(), command }
        : { tool: tool.toLowerCase() }) as ToolInput,
    },
  });
}

function makeToolResult(output: string, isError = false): WireRecord {
  return makeRecord({
    record_type: "tool_result",
    payload: { call_id: "call-1", output, is_error: isError },
  });
}

function makeUserMessage(): WireRecord {
  return makeRecord({ record_type: "user_message", payload: { content: "hello" } });
}

function makeAssistantMessage(): WireRecord {
  return makeRecord({
    record_type: "assistant_message",
    payload: { model: "claude-4", content: [{ type: "text", text: "Sure" }] },
  });
}

function makeReasoning(): WireRecord {
  return makeRecord({
    record_type: "reasoning",
    payload: { summary: ["thinking..."], encrypted: false },
  });
}

// ═══════════════════════════════════════════════════════════════════
// Boundary tables for each filter
// ═══════════════════════════════════════════════════════════════════

describe("TIMELINE_FILTERS — boundary table", () => {
  describe("all", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["user_message",       makeUserMessage(),           true],
      ["assistant_message",  makeAssistantMessage(),      true],
      ["tool_call",          makeToolCall("Bash", "ls"),   true],
      ["tool_result",        makeToolResult("ok"),         true],
      ["reasoning",          makeReasoning(),              true],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["all"]!(r)).toBe(e));
  });

  describe("conversation", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["user_message",       makeUserMessage(),           true],
      ["assistant_message",  makeAssistantMessage(),      true],
      ["tool_call",          makeToolCall("Bash", "ls"),   false],
      ["tool_result",        makeToolResult("ok"),         false],
      ["reasoning",          makeReasoning(),              false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["conversation"]!(r)).toBe(e));
  });

  describe("code", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["Read",               makeToolCall("Read"),                    true],
      ["Edit",               makeToolCall("Edit"),                    true],
      ["Write",              makeToolCall("Write"),                   true],
      ["Glob",               makeToolCall("Glob"),                    true],
      ["Grep",               makeToolCall("Grep"),                    true],
      ["Bash",               makeToolCall("Bash", "ls"),              false],
      ["Agent",              makeToolCall("Agent"),                   false],
      ["user_message",       makeUserMessage(),                      false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["code"]!(r)).toBe(e));
  });

  describe("commands", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["Bash ls",            makeToolCall("Bash", "ls"),              true],
      ["Bash cargo test",    makeToolCall("Bash", "cargo test"),      true],
      ["Bash git push",      makeToolCall("Bash", "git push"),        true],
      ["Read",               makeToolCall("Read"),                    false],
      ["user_message",       makeUserMessage(),                      false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["commands"]!(r)).toBe(e));
  });

  describe("tests", () => {
    const cases: [string, WireRecord, boolean][] = [
      // Commands
      ["cargo test",             makeToolCall("Bash", "cargo test"),                true],
      ["npm test",               makeToolCall("Bash", "npm test"),                  true],
      ["npx vitest",             makeToolCall("Bash", "npx vitest run"),            true],
      ["pytest",                 makeToolCall("Bash", "pytest tests/"),             true],
      // Results
      ["rust test ok",           makeToolResult("test result: ok. 5 passed"),       true],
      ["vitest passed",          makeToolResult("Tests  21 passed"),                true],
      ["FAILED result",          makeToolResult("FAILED. 2 failed, 3 passed"),      true],
      ["1 failed",               makeToolResult("1 failed, 3 passed"),             true],
      // Negatives
      ["cargo build",            makeToolCall("Bash", "cargo build"),               false],
      ["git push",               makeToolCall("Bash", "git push"),                  false],
      ["random output",          makeToolResult("hello world"),                     false],
      ["user_message",           makeUserMessage(),                                false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["tests"]!(r)).toBe(e));
  });

  describe("git", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["git status",         makeToolCall("Bash", "git status"),          true],
      ["git commit",         makeToolCall("Bash", "git commit -m 'x'"),  true],
      ["git push",           makeToolCall("Bash", "git push"),           true],
      ["cd && git diff",     makeToolCall("Bash", "cd /repo && git diff"), true],
      ["cargo test",         makeToolCall("Bash", "cargo test"),         false],
      ["Read",               makeToolCall("Read"),                       false],
      ["user_message",       makeUserMessage(),                         false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["git"]!(r)).toBe(e));
  });

  describe("errors", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["is_error result",       makeToolResult("panic", true),                       true],
      ["error record",          makeRecord({ record_type: "error", payload: { code: "E", message: "fail" } }), true],
      ["compile error[E]",      makeToolResult("error[E0308]: mismatched types"),    true],
      ["TS error",              makeToolResult("TS2345: Argument of type"),           true],
      ["SyntaxError",           makeToolResult("SyntaxError: unexpected token"),      true],
      ["clean result",          makeToolResult("ok"),                                 false],
      ["user_message",          makeUserMessage(),                                   false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["errors"]!(r)).toBe(e));
  });

  describe("thinking", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["reasoning",          makeReasoning(),              true],
      ["tool_call",          makeToolCall("Bash", "ls"),    false],
      ["user_message",       makeUserMessage(),            false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["thinking"]!(r)).toBe(e));
  });

  describe("plans", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["ExitPlanMode",        makeToolCall("ExitPlanMode"),             true],
      ["EnterPlanMode",       makeToolCall("EnterPlanMode"),            true],
      ["Read",                makeToolCall("Read"),                     false],
      ["Bash",                makeToolCall("Bash", "ls"),               false],
      ["user_message",        makeUserMessage(),                       false],
      ["assistant_message",   makeAssistantMessage(),                  false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["plans"]!(r)).toBe(e));
  });

  describe("agents", () => {
    const cases: [string, WireRecord, boolean][] = [
      ["Agent tool",             makeToolCall("Agent"),                    true],
      ["depth 0 Read",           { ...makeToolCall("Read"), depth: 0 },   false],
      ["depth 1 Read",           { ...makeToolCall("Read"), depth: 1 },   false],
      ["depth 2 Read",           { ...makeToolCall("Read"), depth: 2 },   true],
      ["depth 5 Read",           { ...makeToolCall("Read"), depth: 5 },   true],
      ["deep user_message",      { ...makeUserMessage(), depth: 3 },      true],
      ["shallow Bash",           makeToolCall("Bash", "ls"),               false],
    ];
    it.each(cases)("%s → %s", (_d, r, e) => expect(TIMELINE_FILTERS["agents"]!(r)).toBe(e));
  });
});

// ═══════════════════════════════════════════════════════════════════
// Completeness + group structure
// ═══════════════════════════════════════════════════════════════════

describe("TIMELINE_FILTERS — completeness", () => {
  const EXPECTED = ["all", "conversation", "code", "commands", "tests", "git", "errors", "thinking", "plans", "agents"];

  it("should have exactly 10 filters", () => {
    expect(Object.keys(TIMELINE_FILTERS)).toHaveLength(10);
  });

  it.each(EXPECTED)("should have filter: %s", (name) => {
    expect(TIMELINE_FILTERS).toHaveProperty(name);
    expect(typeof TIMELINE_FILTERS[name]).toBe("function");
  });
});

describe("TIMELINE_FILTERS — filter groups", () => {
  it("every filter should appear in exactly one group", () => {
    const allGroupFilters = FILTER_GROUPS.flatMap((g) => g.filters);
    for (const name of Object.keys(TIMELINE_FILTERS)) {
      expect(allGroupFilters.filter((f) => f === name)).toHaveLength(1);
    }
  });
});
