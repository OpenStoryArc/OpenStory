// test.ts — Tests for the eval-apply detector.
//
// TDD: these were written before the detector.
// Same method as the Scheme code. Same spirit.
//
// Run: npx tsx test.ts

import { feed, buildSession } from "./eval-apply-detector.js";
import { initialState, type ApiRecord, type DetectorState, type StructuralEvent } from "./types.js";

// ─────────────────────────────────────────────
// Test framework (minimal, same spirit as 00-prelude.scm)
// ─────────────────────────────────────────────

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
// Record factories
// ─────────────────────────────────────────────

let seq = 0;
function makeRecord(type: string, payload: any = {}, depth = 0): ApiRecord {
  return {
    id: `test-${++seq}`,
    record_type: type as any,
    seq,
    session_id: "test-session",
    timestamp: `2026-04-03T0${depth}:00:${String(seq).padStart(2, "0")}Z`,
    depth,
    parent_uuid: null,
    is_sidechain: false,
    payload,
    payload_bytes: 0,
    truncated: false,
  };
}

function userMsg(content: string, depth = 0) {
  return makeRecord("user_message", { content }, depth);
}

function assistantMsg(content: string, depth = 0) {
  return makeRecord("assistant_message", {
    message: { content: [{ type: "text", text: content }] },
  }, depth);
}

function toolCall(name: string, input: any = {}, depth = 0) {
  return makeRecord("tool_call", { name, call_id: `call-${seq}`, input }, depth);
}

function toolResult(output: string, depth = 0) {
  return makeRecord("tool_result", { call_id: `call-${seq}`, output, is_error: false }, depth);
}

function turnEnd(reason = "end_turn", depth = 0) {
  return makeRecord("turn_end", { reason, duration_ms: 100 }, depth);
}

function systemEvent(subtype: string, depth = 0) {
  return makeRecord("system_event", { subtype }, depth);
}

// Helper: feed a sequence of records, collect all events
function feedAll(records: ApiRecord[]): { state: DetectorState; events: StructuralEvent[] } {
  let state = initialState();
  const allEvents: StructuralEvent[] = [];
  for (const record of records) {
    const [newState, events] = feed(state, record);
    state = newState;
    allEvents.push(...events);
  }
  return { state, events: allEvents };
}

// ─────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────

console.log("\n── Simple text response ──");

test("user message increments env", () => {
  const [state] = feed(initialState(), userMsg("hello"));
  assertEqual(state.envSize, 1, "env should be 1");
});

test("assistant message emits eval and increments env", () => {
  const { state, events } = feedAll([
    userMsg("hello"),
    assistantMsg("hi there"),
  ]);
  assertEqual(state.envSize, 2, "env should be 2");
  assert(events.some(e => e.phase === "eval"), "should emit eval");
});

test("turn_end emits turn_end event", () => {
  const { events } = feedAll([
    userMsg("hello"),
    assistantMsg("hi"),
    turnEnd("end_turn"),
  ]);
  assert(events.some(e => e.phase === "turn_end"), "should emit turn_end");
  const te = events.find(e => e.phase === "turn_end")!;
  assertEqual(te.turnNumber, 1, "turn number should be 1");
  assert(te.stopReason === "end_turn", "stop reason should be end_turn");
});

test("simple response has no apply events", () => {
  const { events } = feedAll([
    userMsg("hello"),
    assistantMsg("hi"),
    turnEnd("end_turn"),
  ]);
  assert(!events.some(e => e.phase === "apply"), "should have no apply events");
});

console.log("\n── Tool use ──");

test("tool_call emits apply event", () => {
  const { events } = feedAll([
    userMsg("list files"),
    assistantMsg("checking..."),
    toolCall("Bash", { command: "ls" }),
  ]);
  assert(events.some(e => e.phase === "apply"), "should emit apply");
  const apply = events.find(e => e.phase === "apply")!;
  assertEqual(apply.toolName, "Bash", "tool name should be Bash");
});

test("tool_result increments env", () => {
  const { state } = feedAll([
    userMsg("list files"),
    assistantMsg("checking"),
    toolCall("Bash"),
    toolResult("file1.txt"),
  ]);
  // user + assistant + tool_result = 3
  assertEqual(state.envSize, 3, "env should be 3");
});

test("full tool cycle: eval then apply then turn_end", () => {
  const { events } = feedAll([
    userMsg("list files"),
    assistantMsg("let me check"),
    toolCall("Bash", { command: "ls" }),
    toolResult("file1.txt\nfile2.txt"),
    turnEnd("tool_use"),
  ]);
  const phases = events.map(e => e.phase);
  assert(phases.includes("eval"), "should have eval");
  assert(phases.includes("apply"), "should have apply");
  assert(phases.includes("turn_end"), "should have turn_end");
});

console.log("\n── Multiple turns ──");

test("two turns have correct turn numbers", () => {
  const { events } = feedAll([
    userMsg("hello"),
    assistantMsg("let me check"),
    toolCall("Bash"),
    toolResult("result"),
    turnEnd("tool_use"),
    assistantMsg("here's what I found"),
    turnEnd("end_turn"),
  ]);
  const turnEnds = events.filter(e => e.phase === "turn_end");
  assertEqual(turnEnds.length, 2, "should have 2 turn_end events");
  assertEqual(turnEnds[0].turnNumber, 1, "first turn is 1");
  assertEqual(turnEnds[1].turnNumber, 2, "second turn is 2");
});

test("env grows across turns", () => {
  const { events } = feedAll([
    userMsg("hello"),
    assistantMsg("checking"),
    toolCall("Bash"),
    toolResult("result"),
    turnEnd("tool_use"),
    assistantMsg("done"),
    turnEnd("end_turn"),
  ]);
  const turnEnds = events.filter(e => e.phase === "turn_end");
  assert(turnEnds[1].envSize > turnEnds[0].envSize, "env should grow");
});

console.log("\n── Scope nesting (Agent tool) ──");

test("Agent tool_call emits scope_open", () => {
  const { events } = feedAll([
    userMsg("delegate this"),
    assistantMsg("spawning agent"),
    toolCall("Agent", { prompt: "do something" }),
  ]);
  assert(events.some(e => e.phase === "scope_open"), "should emit scope_open");
});

test("Agent tool_result emits scope_close", () => {
  const { events } = feedAll([
    toolCall("Agent", { prompt: "task" }),
    toolResult("agent result"),
  ]);
  assert(events.some(e => e.phase === "scope_close"), "should emit scope_close");
});

test("non-Agent tools do not affect scope", () => {
  const { events } = feedAll([
    toolCall("Bash", { command: "ls" }),
    toolResult("files"),
  ]);
  assert(!events.some(e => e.phase === "scope_open"), "no scope_open for Bash");
  assert(!events.some(e => e.phase === "scope_close"), "no scope_close for Bash");
});

console.log("\n── Compaction (GC) ──");

test("system.compact emits compact event", () => {
  const { events } = feedAll([
    systemEvent("system.compact"),
  ]);
  assert(events.some(e => e.phase === "compact"), "should emit compact");
});

console.log("\n── Session builder ──");

test("buildSession groups events into turns anchored on turn_end", () => {
  const records = [
    userMsg("hello"),
    assistantMsg("let me check"),
    toolCall("Bash", { command: "ls" }),
    toolResult("files"),
    assistantMsg("here you go"),
    turnEnd("end_turn"),
  ];
  // One turn_end = one turn (even though there are 2 evals inside)
  const session = buildSession("test-session", "test label", "claude", records);
  assertEqual(session.turns.length, 1, "one turn_end = one turn");
  assertEqual(session.turns[0].isTerminal, true, "end_turn is terminal");
  assertEqual(session.totalApplies, 1, "one apply");
  assertEqual(session.totalEvals, 2, "two evals within the turn");
  assert(session.turns[0].eval!.content.includes("[2 eval cycles]"),
    "should note multiple eval cycles");
});

test("two turn_ends = two turns", () => {
  const records = [
    userMsg("hello"),
    assistantMsg("checking"),
    toolCall("Bash"),
    toolResult("result"),
    turnEnd("tool_use"),
    userMsg("thanks"),
    assistantMsg("done"),
    turnEnd("end_turn"),
  ];
  const session = buildSession("test", "test", "claude", records);
  assertEqual(session.turns.length, 2, "two turn_ends = two turns");
  assertEqual(session.turns[0].turnNumber, 1, "first turn is 1");
  assertEqual(session.turns[1].turnNumber, 2, "second turn is 2");
});

console.log("\n── Rich turn data ──");

test("turns capture human messages", () => {
  const records = [
    userMsg("What is a coalgebra?"),
    assistantMsg("It's the dual of an algebra."),
    turnEnd("end_turn"),
  ];
  const session = buildSession("test", "test", "claude", records);
  assert(session.turns[0].human !== null, "should have human");
  assert(session.turns[0].human!.content.includes("coalgebra"), "human content");
});

test("turns capture thinking blocks", () => {
  const records = [
    userMsg("explain"),
    makeRecord("reasoning", { content: "Let me think step by step...", summary: "step by step" }),
    assistantMsg("Here's the answer"),
    turnEnd("end_turn"),
  ];
  const session = buildSession("test", "test", "claude", records);
  assert(session.turns[0].thinking !== null, "should have thinking");
  assert(session.turns[0].thinking!.tokens > 0, "thinking has tokens");
});

test("env delta tracks growth per turn", () => {
  const records = [
    userMsg("hello"),
    assistantMsg("checking"),
    toolCall("Bash"),
    toolResult("result"),
    turnEnd("tool_use"),
    assistantMsg("done"),
    turnEnd("end_turn"),
  ];
  const session = buildSession("test", "test", "claude", records);
  assert(session.turns[0].envDelta > 0, "first turn should grow env");
  assert(session.turns[1].envDelta > 0, "second turn should grow env");
});

// ─────────────────────────────────────────────
// Summary
// ─────────────────────────────────────────────

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
