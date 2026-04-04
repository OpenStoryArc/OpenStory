// domain-test.ts — Tests for the domain event layer.
//
// Domain events are FACTS. Deterministic. No heuristics.
// A Write that returns "File created successfully" is always FileCreated.
// An Edit is always FileModified. A Read is always FileRead.
//
// The interpretation layer (sentences) sits above this.
// This layer is the honest contract.
//
// Run: npx tsx domain-test.ts

import { toDomainEvent, buildDomainTurn, type DomainEvent } from "./domain.js";
import type { StructuralTurn } from "./types.js";

let passed = 0;
let failed = 0;

function test(name: string, fn: () => void) {
  try {
    fn();
    passed++;
    console.log(`  pass: ${name}`);
  } catch (e: any) {
    failed++;
    console.log(`  FAIL: ${name} — ${e.message}`);
  }
}

function assertEqual<T>(actual: T, expected: T, msg: string) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${msg}\n    expected: ${JSON.stringify(expected)}\n    got:      ${JSON.stringify(actual)}`);
  }
}

function assert(val: boolean, msg: string) {
  if (!val) throw new Error(msg);
}

// ─────────────────────────────────────────────
// Domain event mapping — deterministic
// ─────────────────────────────────────────────

console.log("\n── Write events ──");

test("Write with 'created successfully' → FileCreated", () => {
  const e = toDomainEvent("Write", "/scheme/01-types.scm",
    "File created successfully at: /scheme/01-types.scm");
  assertEqual(e.type, "FileCreated", "type");
  assertEqual(e.path, "/scheme/01-types.scm", "path");
});

test("Write with 'updated successfully' → FileModified", () => {
  const e = toDomainEvent("Write", "/scheme/01-types.scm",
    "The file /scheme/01-types.scm has been updated successfully.");
  assertEqual(e.type, "FileModified", "type");
});

test("Write with error → FileWriteFailed", () => {
  const e = toDomainEvent("Write", "/readonly/file.txt",
    "Permission denied", true);
  assertEqual(e.type, "FileWriteFailed", "type");
});

console.log("\n── Edit events ──");

test("Edit → always FileModified", () => {
  const e = toDomainEvent("Edit", "/README.md", "updated successfully");
  assertEqual(e.type, "FileModified", "type");
  assertEqual(e.path, "/README.md", "path");
});

test("Edit with error → FileWriteFailed", () => {
  const e = toDomainEvent("Edit", "/README.md", "old_string not found", true);
  assertEqual(e.type, "FileWriteFailed", "type");
});

console.log("\n── Read events ──");

test("Read → FileRead", () => {
  const e = toDomainEvent("Read", "/src/main.rs", "fn main() {}");
  assertEqual(e.type, "FileRead", "type");
  assertEqual(e.path, "/src/main.rs", "path");
});

test("Read with error → FileReadFailed", () => {
  const e = toDomainEvent("Read", "/nonexistent.rs", "File not found", true);
  assertEqual(e.type, "FileReadFailed", "type");
});

console.log("\n── Search events ──");

test("Grep → SearchPerformed", () => {
  const e = toDomainEvent("Grep", "pattern: TODO", "src/main.rs: [match]");
  assertEqual(e.type, "SearchPerformed", "type");
  assert("pattern" in e, "has pattern");
});

test("Glob → SearchPerformed", () => {
  const e = toDomainEvent("Glob", "**/*.scm", "01-types.scm\n02-stream.scm");
  assertEqual(e.type, "SearchPerformed", "type");
});

test("WebSearch → SearchPerformed", () => {
  const e = toDomainEvent("WebSearch", "MIT lambda papers", "results...");
  assertEqual(e.type, "SearchPerformed", "type");
});

console.log("\n── Bash events ──");

test("Bash → CommandExecuted with success", () => {
  const e = toDomainEvent("Bash", "ls -la", "file1.txt\nfile2.txt");
  assertEqual(e.type, "CommandExecuted", "type");
  assert("command" in e, "has command");
  assertEqual((e as any).succeeded, true, "succeeded");
});

test("Bash with error → CommandExecuted failed", () => {
  const e = toDomainEvent("Bash", "cargo test", "test failed", true);
  assertEqual(e.type, "CommandExecuted", "type");
  assertEqual((e as any).succeeded, false, "failed");
});

console.log("\n── Agent events ──");

test("Agent → SubAgentSpawned", () => {
  const e = toDomainEvent("Agent", "Explore the codebase", "result...");
  assertEqual(e.type, "SubAgentSpawned", "type");
  assert("description" in e, "has description");
});

console.log("\n── Domain turn building ──");

function makeTurn(overrides: Partial<StructuralTurn> = {}): StructuralTurn {
  return {
    turnNumber: 1, scopeDepth: 0, human: null, thinking: null,
    eval: null, applies: [], envSize: 5, envDelta: 3,
    stopReason: "end_turn", isTerminal: true,
    timestamp: "2026-04-03T01:00:00Z", durationMs: null,
    ...overrides,
  };
}

test("domain turn tracks files created", () => {
  const turn = makeTurn({
    applies: [
      { toolName: "Write", inputSummary: "/scheme/01-types.scm",
        outputSummary: "File created successfully at: /scheme/01-types.scm",
        isAgent: false, isError: false },
      { toolName: "Write", inputSummary: "/scheme/02-stream.scm",
        outputSummary: "File created successfully at: /scheme/02-stream.scm",
        isAgent: false, isError: false },
    ],
  });
  const dt = buildDomainTurn(turn);
  assertEqual(dt.aggregate.filesCreated.length, 2, "2 files created");
  assert(dt.aggregate.filesCreated.includes("01-types.scm"), "has 01-types.scm");
});

test("domain turn tracks files modified", () => {
  const turn = makeTurn({
    applies: [
      { toolName: "Edit", inputSummary: "/README.md",
        outputSummary: "updated successfully",
        isAgent: false, isError: false },
    ],
  });
  const dt = buildDomainTurn(turn);
  assertEqual(dt.aggregate.filesModified.length, 1, "1 file modified");
});

test("domain turn tracks commands run", () => {
  const turn = makeTurn({
    applies: [
      { toolName: "Bash", inputSummary: "chibi-scheme test.ts",
        outputSummary: "11 passed, 0 failed",
        isAgent: false, isError: false },
      { toolName: "Bash", inputSummary: "chibi-scheme test.ts",
        outputSummary: "6 passed, 0 failed",
        isAgent: false, isError: false },
    ],
  });
  const dt = buildDomainTurn(turn);
  assertEqual(dt.aggregate.commandsRun.length, 2, "2 commands");
  assertEqual(dt.aggregate.commandsSucceeded, 2, "2 succeeded");
});

test("domain turn tracks files read", () => {
  const turn = makeTurn({
    applies: [
      { toolName: "Read", inputSummary: "/src/main.rs",
        outputSummary: "fn main() {}", isAgent: false, isError: false },
      { toolName: "Read", inputSummary: "/src/lib.rs",
        outputSummary: "pub mod query;", isAgent: false, isError: false },
    ],
  });
  const dt = buildDomainTurn(turn);
  assertEqual(dt.aggregate.filesRead.length, 2, "2 files read");
});

test("domain turn identifies actors", () => {
  const turn = makeTurn({
    human: { content: "write it in Scheme", timestamp: "" },
    eval: { content: "Let me write it", timestamp: "", decision: "tool_use", tokens: 100 },
  });
  const dt = buildDomainTurn(turn);
  assertEqual(dt.initiator, "human", "human initiated");
  assertEqual(dt.command, "write it in Scheme", "command from human");
});

test("continuation turn has claude as initiator", () => {
  const turn = makeTurn({
    human: null,
    eval: { content: "Continuing...", timestamp: "", decision: "tool_use", tokens: 50 },
  });
  const dt = buildDomainTurn(turn);
  assertEqual(dt.initiator, "claude", "claude continuing");
});

test("domain events list is deterministic", () => {
  const turn = makeTurn({
    applies: [
      { toolName: "Read", inputSummary: "/src/main.rs",
        outputSummary: "fn main() {}", isAgent: false, isError: false },
      { toolName: "Write", inputSummary: "/scheme/01.scm",
        outputSummary: "File created successfully at: /scheme/01.scm",
        isAgent: false, isError: false },
      { toolName: "Bash", inputSummary: "chibi-scheme test.scm",
        outputSummary: "6 passed", isAgent: false, isError: false },
    ],
  });
  const dt = buildDomainTurn(turn);
  assertEqual(dt.events.length, 3, "3 events");
  assertEqual(dt.events[0].type, "FileRead", "first is FileRead");
  assertEqual(dt.events[1].type, "FileCreated", "second is FileCreated");
  assertEqual(dt.events[2].type, "CommandExecuted", "third is CommandExecuted");
});

// ─────────────────────────────────────────────
// Summary
// ─────────────────────────────────────────────

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
