// sentence-test.ts — Tests for the sentence diagramming layer.
//
// Each turn is a sentence. The sentence has grammar.
// The grammar maps to the eval-apply structure.
//
// Run: npx tsx sentence-test.ts

import { buildSentence, classifyTool, type ToolRole } from "./sentence.js";
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
// Turn factories
// ─────────────────────────────────────────────

function makeTurn(overrides: Partial<StructuralTurn> = {}): StructuralTurn {
  return {
    turnNumber: 1,
    scopeDepth: 0,
    human: null,
    thinking: null,
    eval: null,
    applies: [],
    envSize: 5,
    envDelta: 3,
    stopReason: "end_turn",
    isTerminal: true,
    timestamp: "2026-04-03T01:00:00Z",
    durationMs: null,
    ...overrides,
  };
}

// ─────────────────────────────────────────────
// Tool classification
// ─────────────────────────────────────────────

console.log("\n── Tool classification ──");

test("Read is preparatory", () => {
  assertEqual(classifyTool("Read", "/src/main.rs"), "preparatory", "Read");
});

test("Grep is preparatory", () => {
  assertEqual(classifyTool("Grep", "pattern: TODO"), "preparatory", "Grep");
});

test("Write is creative", () => {
  assertEqual(classifyTool("Write", "/scheme/01-types.scm"), "creative", "Write");
});

test("Edit is creative", () => {
  assertEqual(classifyTool("Edit", "/README.md"), "creative", "Edit");
});

test("Bash with test is verificatory", () => {
  assertEqual(classifyTool("Bash", "npx tsx test.ts"), "verificatory", "test");
});

test("Bash with cargo test is verificatory", () => {
  assertEqual(classifyTool("Bash", "cd rs && cargo test"), "verificatory", "cargo test");
});

test("Bash with install is preparatory", () => {
  assertEqual(classifyTool("Bash", "brew install chibi-scheme"), "preparatory", "install");
});

test("Bash with git commit is creative", () => {
  assertEqual(classifyTool("Bash", "git commit -m 'fix'"), "creative", "git commit");
});

test("Bash with git push is creative", () => {
  assertEqual(classifyTool("Bash", "git push fork main"), "creative", "git push");
});

test("Agent is delegatory", () => {
  assertEqual(classifyTool("Agent", "Explore the codebase"), "delegatory", "Agent");
});

test("Bash generic is verificatory", () => {
  assertEqual(classifyTool("Bash", "ls -la"), "verificatory", "generic bash");
});

test("WebSearch is preparatory", () => {
  assertEqual(classifyTool("WebSearch", "MIT lambda papers"), "preparatory", "WebSearch");
});

// ─────────────────────────────────────────────
// Sentence building — simple cases
// ─────────────────────────────────────────────

console.log("\n── Simple sentences ──");

test("text-only turn: Claude answered", () => {
  const turn = makeTurn({
    human: { content: "What is a coalgebra?", timestamp: "" },
    eval: { content: "A coalgebra is the dual of an algebra.", timestamp: "", decision: "text_only", tokens: 500 },
  });
  const s = buildSentence(turn);
  assert(s.verb === "explained" || s.verb === "answered" || s.verb === "responded", `verb should be explanatory, got: ${s.verb}`);
  assert(s.adverbial !== null, "should have adverbial from human msg");
  assert(s.adverbial!.includes("coalgebra"), "adverbial should reference the question");
  assert(s.oneLiner.length > 0, "should produce a one-liner");
});

test("single Read turn", () => {
  const turn = makeTurn({
    human: { content: "Show me the types", timestamp: "" },
    eval: { content: "Here are the types...", timestamp: "", decision: "tool_use", tokens: 200 },
    applies: [
      { toolName: "Read", inputSummary: "/src/core/lib.rs", outputSummary: "pub enum ContentBlock...", isAgent: false, isError: false },
    ],
  });
  const s = buildSentence(turn);
  assert(s.verb === "read" || s.verb === "examined", `verb: ${s.verb}`);
  assert(s.object.includes("lib.rs") || s.object.includes("file"), `object: ${s.object}`);
});

test("Write-heavy turn", () => {
  const turn = makeTurn({
    human: { content: "write it in Scheme", timestamp: "" },
    eval: { content: "Let me write the code", timestamp: "", decision: "tool_use", tokens: 100 },
    applies: [
      { toolName: "Read", inputSummary: "/src/query/lib.rs", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Read", inputSummary: "/src/api/lib.rs", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Write", inputSummary: "/scheme/01-types.scm", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Write", inputSummary: "/scheme/02-stream.scm", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Write", inputSummary: "/scheme/03-tools.scm", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Bash", inputSummary: "chibi-scheme test.scm", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Bash", inputSummary: "chibi-scheme test.scm", outputSummary: "", isAgent: false, isError: false },
    ],
  });
  const s = buildSentence(turn);
  assert(s.verb === "wrote", `verb should be 'wrote', got: ${s.verb}`);
  assert(s.object.includes("3") || s.object.includes("Scheme"), `object should mention files: ${s.object}`);
  assert(s.subordinates.length > 0, "should have subordinate clauses");
  // Should have preparatory (reads) and verificatory (bash test)
  const roles = s.subordinates.map(c => c.role);
  assert(roles.includes("preparatory"), "should have preparatory clause");
  assert(roles.includes("verificatory"), "should have verificatory clause");
});

console.log("\n── Subordinate clauses ──");

test("Agent delegation creates delegatory clause", () => {
  const turn = makeTurn({
    human: { content: "tell me about it", timestamp: "" },
    eval: { content: "Let me explore", timestamp: "", decision: "tool_use", tokens: 50 },
    applies: [
      { toolName: "Agent", inputSummary: "Explore claurst project", outputSummary: "", isAgent: true, isError: false },
    ],
  });
  const s = buildSentence(turn);
  assert(s.verb === "delegated" || s.verb === "explored", `verb: ${s.verb}`);
  const delegatory = s.subordinates.find(c => c.role === "delegatory");
  if (s.verb !== "delegated") {
    assert(delegatory !== undefined, "should have delegatory clause");
  }
});

test("mixed turn has ordered subordinates", () => {
  const turn = makeTurn({
    human: { content: "fix the bug", timestamp: "" },
    eval: { content: "Fixed", timestamp: "", decision: "tool_use", tokens: 100 },
    applies: [
      { toolName: "Read", inputSummary: "main.rs", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Read", inputSummary: "lib.rs", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Edit", inputSummary: "main.rs", outputSummary: "", isAgent: false, isError: false },
      { toolName: "Bash", inputSummary: "cargo test", outputSummary: "", isAgent: false, isError: false },
    ],
  });
  const s = buildSentence(turn);
  // Subordinates should be ordered: preparatory before verificatory
  const roles = s.subordinates.map(c => c.role);
  if (roles.includes("preparatory") && roles.includes("verificatory")) {
    const prepIdx = roles.indexOf("preparatory");
    const verIdx = roles.indexOf("verificatory");
    assert(prepIdx < verIdx, "preparatory before verificatory");
  }
});

console.log("\n── One-liner composition ──");

test("one-liner includes all parts", () => {
  const turn = makeTurn({
    human: { content: "What files are here?", timestamp: "" },
    eval: { content: "Here are the files.", timestamp: "", decision: "tool_use", tokens: 100 },
    applies: [
      { toolName: "Bash", inputSummary: "ls", outputSummary: "main.rs\nlib.rs", isAgent: false, isError: false },
    ],
  });
  const s = buildSentence(turn);
  assert(s.oneLiner.includes("Claude"), "one-liner has subject");
  assert(s.oneLiner.length > 10, "one-liner is substantial");
});

test("turn with no human message still works", () => {
  const turn = makeTurn({
    human: null,
    eval: { content: "Continuing...", timestamp: "", decision: "tool_use", tokens: 50 },
    applies: [
      { toolName: "Bash", inputSummary: "git status", outputSummary: "", isAgent: false, isError: false },
    ],
  });
  const s = buildSentence(turn);
  assert(s.adverbial === null || s.adverbial.includes("continuing"), `adverbial: ${s.adverbial}`);
  assert(s.oneLiner.length > 0, "one-liner still produced");
});

test("turn with no tools is pure response", () => {
  const turn = makeTurn({
    human: { content: "fuck me that is cool", timestamp: "" },
    eval: { content: "Yeah — it's the dual of an algebra.", timestamp: "", decision: "text_only", tokens: 800 },
    applies: [],
  });
  const s = buildSentence(turn);
  assert(!s.verb.includes("wrote") && !s.verb.includes("read"), `should not be action verb: ${s.verb}`);
  assertEqual(s.subordinates.length, 0, "no subordinates for pure text");
});

// ─────────────────────────────────────────────
// Summary
// ─────────────────────────────────────────────

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
