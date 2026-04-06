/**
 * Spec: extractCycles — derive eval-apply cycles from session records.
 *
 * The recursive unit of agent work. Each cycle is:
 *   EVAL (model concludes something) → APPLY* (zero or more tool calls)
 *
 * A terminal cycle has 0 tools — the model decided to stop.
 * A non-terminal cycle has 1+ tools — the model dispatched work.
 *
 * Statistical properties (from analyze_eval_apply_shape.py):
 *   - Subagents: 7-32 cycles, always 1 prompt, last is always terminal
 *   - Tools per cycle: min=1 max=16 avg=2.2
 *   - Tool calls always equal tool results
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { extractCycles } from "@/lib/eval-apply";
import type { WireRecord } from "@/types/wire-record";

// ═══════════════════════════════════════════════════════════════════
// Test data factory — synthetic records matching real statistics
// ═══════════════════════════════════════════════════════════════════

let seq = 0;

function makeRecord(record_type: string, overrides: Partial<WireRecord> = {}): WireRecord {
  seq++;
  return {
    id: `evt-${seq}`,
    seq,
    session_id: "agent-test123",
    timestamp: `2026-01-01T00:00:${String(seq).padStart(2, "0")}Z`,
    record_type: record_type as any,
    payload: {},
    agent_id: null,
    is_sidechain: true,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 100,
    ...overrides,
  };
}

function userMessage(text: string): WireRecord {
  return makeRecord("user_message", {
    payload: { content: [{ type: "text", text }] },
  });
}

function assistantMessage(text: string): WireRecord {
  return makeRecord("assistant_message", {
    payload: { content: [{ type: "text", text }], stop_reason: "end_turn" },
  });
}

function toolCall(name: string, input: Record<string, string> = {}): WireRecord {
  return makeRecord("tool_call", {
    payload: { name, input, call_id: `call-${seq}`, raw_input: {} } as any,
  });
}

function toolResult(output: string = "ok"): WireRecord {
  return makeRecord("tool_result", {
    payload: { output, is_error: false, call_id: `call-${seq}` } as any,
  });
}

// ═══════════════════════════════════════════════════════════════════
// Basic cycle extraction
// ═══════════════════════════════════════════════════════════════════

describe("extractCycles", () => {
  it("should extract a single terminal cycle (text-only response)", () => {
    scenario(
      () => [
        userMessage("What is eval-apply?"),
        assistantMessage("It's a recursive evaluation model."),
      ],
      (records) => extractCycles(records),
      (cycles) => {
        expect(cycles).toHaveLength(1);
        expect(cycles[0]!.isTerminal).toBe(true);
        expect(cycles[0]!.evalText).toContain("recursive evaluation");
        expect(cycles[0]!.tools).toHaveLength(0);
        expect(cycles[0]!.cycleNumber).toBe(1);
      },
    );
  });

  it("should extract one cycle with tools", () => {
    scenario(
      () => [
        userMessage("Read the file"),
        assistantMessage("Let me read it."),
        toolCall("Read", { file_path: "/src/main.rs" }),
        toolResult("fn main() {}"),
        assistantMessage("Here's what I found."),
      ],
      (records) => extractCycles(records),
      (cycles) => {
        expect(cycles).toHaveLength(2);
        // Cycle 1: eval + 1 tool
        expect(cycles[0]!.isTerminal).toBe(false);
        expect(cycles[0]!.evalText).toContain("Let me read");
        expect(cycles[0]!.tools).toHaveLength(1);
        expect(cycles[0]!.tools[0]!.name).toBe("Read");
        // Cycle 2: terminal
        expect(cycles[1]!.isTerminal).toBe(true);
        expect(cycles[1]!.evalText).toContain("what I found");
      },
    );
  });

  it("should handle multiple tools per cycle", () => {
    scenario(
      () => [
        userMessage("Explore the codebase"),
        assistantMessage("Let me look around."),
        toolCall("Bash", { command: "ls" }),
        toolResult("file1.rs\nfile2.rs"),
        toolCall("Read", { file_path: "/file1.rs" }),
        toolResult("contents1"),
        toolCall("Read", { file_path: "/file2.rs" }),
        toolResult("contents2"),
        assistantMessage("I found two files."),
      ],
      (records) => extractCycles(records),
      (cycles) => {
        expect(cycles).toHaveLength(2);
        expect(cycles[0]!.tools).toHaveLength(3);
        expect(cycles[0]!.tools.map(t => t.name)).toEqual(["Bash", "Read", "Read"]);
        expect(cycles[1]!.isTerminal).toBe(true);
      },
    );
  });

  it("should extract many cycles (realistic subagent shape)", () => {
    // Synthetic subagent: 1 prompt, 5 tool cycles, 1 terminal
    const records: WireRecord[] = [
      userMessage("Explore the patterns module"),
    ];
    for (let i = 0; i < 5; i++) {
      records.push(assistantMessage(`Let me check step ${i}...`));
      const toolCount = [2, 3, 1, 4, 2][i]!;
      for (let j = 0; j < toolCount; j++) {
        records.push(toolCall("Read", { file_path: `/file${i}_${j}.rs` }));
        records.push(toolResult(`contents of file${i}_${j}`));
      }
    }
    records.push(assistantMessage("Here's my complete analysis."));

    const cycles = extractCycles(records);
    expect(cycles).toHaveLength(6);
    expect(cycles[0]!.tools).toHaveLength(2);
    expect(cycles[1]!.tools).toHaveLength(3);
    expect(cycles[4]!.tools).toHaveLength(2);
    expect(cycles[5]!.isTerminal).toBe(true);
    expect(cycles[5]!.tools).toHaveLength(0);
  });

  it("should number cycles sequentially", () => {
    const records = [
      userMessage("go"),
      assistantMessage("step 1"),
      toolCall("Bash", { command: "ls" }),
      toolResult("ok"),
      assistantMessage("step 2"),
      toolCall("Read", { file_path: "/a.rs" }),
      toolResult("ok"),
      assistantMessage("done"),
    ];
    const cycles = extractCycles(records);
    expect(cycles.map(c => c.cycleNumber)).toEqual([1, 2, 3]);
  });

  it("should return empty array for no records", () => {
    expect(extractCycles([])).toEqual([]);
  });

  it("should return empty array for records with no assistant messages", () => {
    const records = [userMessage("hello")];
    expect(extractCycles(records)).toEqual([]);
  });

  it("should handle Agent tool calls", () => {
    const records = [
      userMessage("delegate this"),
      assistantMessage("I'll spawn an agent."),
      toolCall("Agent", { description: "Explore codebase" }),
      toolResult("Agent completed"),
      assistantMessage("The agent found the answer."),
    ];
    const cycles = extractCycles(records);
    expect(cycles).toHaveLength(2);
    expect(cycles[0]!.tools[0]!.name).toBe("Agent");
    expect(cycles[0]!.tools[0]!.summary).toBe("Explore codebase");
  });

  // Statistical invariant: last cycle is always terminal
  it("should always have terminal last cycle", () => {
    const records = [
      userMessage("go"),
      assistantMessage("checking"),
      toolCall("Bash", { command: "ls" }),
      toolResult("ok"),
      assistantMessage("more checking"),
      toolCall("Read", { file_path: "/a.rs" }),
      toolResult("ok"),
      assistantMessage("all done"),
    ];
    const cycles = extractCycles(records);
    expect(cycles[cycles.length - 1]!.isTerminal).toBe(true);
  });

  // Statistical invariant: tool_calls == tool_results
  it("should pair tools correctly even with mixed types", () => {
    const records = [
      userMessage("go"),
      assistantMessage("let me check"),
      toolCall("Read", { file_path: "/a.rs" }),
      toolResult("contents a"),
      toolCall("Grep", { pattern: "TODO" }),
      toolResult("3 matches"),
      toolCall("Bash", { command: "cargo test" }),
      toolResult("ok"),
      assistantMessage("done"),
    ];
    const cycles = extractCycles(records);
    expect(cycles[0]!.tools).toHaveLength(3);
    expect(cycles[0]!.tools.map(t => t.name)).toEqual(["Read", "Grep", "Bash"]);
  });
});
