/**
 * Spec: Story data surfacing — domain facts, event IDs, and session IDs
 * on every TurnCard.
 *
 * Tests the pure functions that extract per-turn domain facts and
 * map event IDs to individual applies, so every card shows what
 * changed and which CloudEvents drove the change.
 *
 * These are the "always visible" data: no detail expand needed.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import type { PatternView } from "@/types/wire-record";
import { turnDomainFacts, turnEventMap, agentSessionTurns } from "@/lib/story";
import type { DomainFact } from "@/lib/domain-facts";

// ═══════════════════════════════════════════════════════════════════
// Test data factory — with realistic ToolOutcome values
// ═══════════════════════════════════════════════════════════════════

function makeSentenceWithOutcomes(overrides: {
  turn?: number;
  session_id?: string;
  events?: string[];
  applies?: Array<{
    tool_name: string;
    input_summary: string;
    output_summary: string;
    is_error: boolean;
    is_agent: boolean;
    tool_outcome: { type: string; path?: string; command?: string; succeeded?: boolean; pattern?: string; source?: string; description?: string } | null;
  }>;
} = {}): PatternView {
  const turn = overrides.turn ?? 1;
  return {
    type: "turn.sentence",
    label: "Claude wrote something → answered",
    session_id: overrides.session_id ?? "sess-abc123",
    events: overrides.events ?? [`evt-${turn}-1`, `evt-${turn}-2`, `evt-${turn}-3`],
    metadata: {
      turn,
      subject: "Claude",
      verb: "wrote",
      object: "something",
      adverbial: null,
      predicate: "answered",
      subordinates: [],
      scope_depth: 0,
      env_size: 10,
      env_delta: 3,
      stop_reason: "end_turn",
      is_terminal: true,
      duration_ms: 2500,
      human: { content: "fix the bug", timestamp: "2026-04-05T10:00:00Z" },
      thinking: null,
      eval: { content: "I'll fix it", decision: "tool_use", timestamp: "2026-04-05T10:00:01Z" },
      applies: overrides.applies ?? [
        {
          tool_name: "Read",
          input_summary: "/src/lib/story.ts",
          output_summary: "file contents...",
          is_error: false,
          is_agent: false,
          tool_outcome: { type: "FileRead", path: "/src/lib/story.ts" },
        },
        {
          tool_name: "Edit",
          input_summary: "/src/lib/story.ts",
          output_summary: "✓ 5 lines modified",
          is_error: false,
          is_agent: false,
          tool_outcome: { type: "FileModified", path: "/src/lib/story.ts" },
        },
        {
          tool_name: "Bash",
          input_summary: "npm test",
          output_summary: "✓ All tests pass",
          is_error: false,
          is_agent: false,
          tool_outcome: { type: "CommandExecuted", command: "npm test", succeeded: true },
        },
      ],
    },
  };
}

// ═══════════════════════════════════════════════════════════════════
// Feature: turnDomainFacts — extract domain facts from a turn pattern
// ═══════════════════════════════════════════════════════════════════

describe("turnDomainFacts", () => {
  it("should extract domain facts from applies with tool_outcome", () => {
    scenario(
      () => makeSentenceWithOutcomes(),
      (pattern) => turnDomainFacts(pattern),
      (facts) => {
        expect(facts).toHaveLength(3);
        expect(facts[0]!.kind).toBe("modified");  // modified sorts before read
        expect(facts[1]!.kind).toBe("read");
        expect(facts[2]!.kind).toBe("command_ok");
      },
    );
  });

  it("should return empty array when no applies have tool_outcome", () => {
    scenario(
      () => makeSentenceWithOutcomes({
        applies: [
          { tool_name: "Read", input_summary: "", output_summary: "", is_error: false, is_agent: false, tool_outcome: null },
        ],
      }),
      (pattern) => turnDomainFacts(pattern),
      (facts) => expect(facts).toHaveLength(0),
    );
  });

  it("should return empty array when there are no applies", () => {
    scenario(
      () => makeSentenceWithOutcomes({ applies: [] }),
      (pattern) => turnDomainFacts(pattern),
      (facts) => expect(facts).toHaveLength(0),
    );
  });

  it("should deduplicate repeated reads of the same file", () => {
    scenario(
      () => makeSentenceWithOutcomes({
        applies: [
          { tool_name: "Read", input_summary: "/a.rs", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "FileRead", path: "/a.rs" } },
          { tool_name: "Read", input_summary: "/a.rs", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "FileRead", path: "/a.rs" } },
        ],
      }),
      (pattern) => turnDomainFacts(pattern),
      (facts) => expect(facts).toHaveLength(1),
    );
  });

  it("should include FileCreated, FileModified, SearchPerformed, SubAgentSpawned", () => {
    scenario(
      () => makeSentenceWithOutcomes({
        applies: [
          { tool_name: "Write", input_summary: "/new.ts", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "FileCreated", path: "/new.ts" } },
          { tool_name: "Grep", input_summary: "TODO", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "SearchPerformed", pattern: "TODO", source: "filesystem" } },
          { tool_name: "Agent", input_summary: "explore", output_summary: "", is_error: false, is_agent: true, tool_outcome: { type: "SubAgentSpawned", description: "Explore codebase" } },
        ],
      }),
      (pattern) => turnDomainFacts(pattern),
      (facts) => {
        const kinds = facts.map(f => f.kind);
        expect(kinds).toContain("created");
        expect(kinds).toContain("search");
        expect(kinds).toContain("agent");
      },
    );
  });

  it("should handle error outcomes (FileWriteFailed)", () => {
    scenario(
      () => makeSentenceWithOutcomes({
        applies: [
          { tool_name: "Write", input_summary: "/readonly.txt", output_summary: "Permission denied", is_error: true, is_agent: false, tool_outcome: { type: "FileWriteFailed", path: "/readonly.txt" } },
        ],
      }),
      (pattern) => turnDomainFacts(pattern),
      (facts) => {
        expect(facts).toHaveLength(1);
        expect(facts[0]!.kind).toBe("error");
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// Feature: turnEventMap — pair applies with event IDs and facts
// ═══════════════════════════════════════════════════════════════════

describe("turnEventMap", () => {
  it("should pair each apply with its domain fact", () => {
    scenario(
      () => makeSentenceWithOutcomes({
        events: ["evt-human", "evt-thinking", "evt-eval", "evt-read", "evt-read-result", "evt-edit", "evt-edit-result", "evt-bash", "evt-bash-result", "evt-turn-end"],
        applies: [
          { tool_name: "Read", input_summary: "/a.ts", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "FileRead", path: "/a.ts" } },
          { tool_name: "Edit", input_summary: "/a.ts", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "FileModified", path: "/a.ts" } },
          { tool_name: "Bash", input_summary: "npm test", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "CommandExecuted", command: "npm test", succeeded: true } },
        ],
      }),
      (pattern) => turnEventMap(pattern),
      (entries) => {
        expect(entries).toHaveLength(3);
        expect(entries[0]!.tool_name).toBe("Read");
        expect(entries[0]!.fact).not.toBeNull();
        expect(entries[0]!.fact!.kind).toBe("read");
        expect(entries[1]!.tool_name).toBe("Edit");
        expect(entries[1]!.fact!.kind).toBe("modified");
        expect(entries[2]!.tool_name).toBe("Bash");
        expect(entries[2]!.fact!.kind).toBe("command_ok");
      },
    );
  });

  it("should return null fact when tool_outcome is missing", () => {
    scenario(
      () => makeSentenceWithOutcomes({
        applies: [
          { tool_name: "Read", input_summary: "/a.ts", output_summary: "", is_error: false, is_agent: false, tool_outcome: null },
        ],
      }),
      (pattern) => turnEventMap(pattern),
      (entries) => {
        expect(entries).toHaveLength(1);
        expect(entries[0]!.tool_name).toBe("Read");
        expect(entries[0]!.fact).toBeNull();
      },
    );
  });

  it("should return empty array for turn with no applies", () => {
    scenario(
      () => makeSentenceWithOutcomes({ applies: [] }),
      (pattern) => turnEventMap(pattern),
      (entries) => expect(entries).toHaveLength(0),
    );
  });

  it("should preserve apply order", () => {
    scenario(
      () => makeSentenceWithOutcomes({
        applies: [
          { tool_name: "Grep", input_summary: "TODO", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "SearchPerformed", pattern: "TODO", source: "filesystem" } },
          { tool_name: "Write", input_summary: "/new.ts", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "FileCreated", path: "/new.ts" } },
          { tool_name: "Bash", input_summary: "npm test", output_summary: "", is_error: false, is_agent: false, tool_outcome: { type: "CommandExecuted", command: "npm test", succeeded: true } },
        ],
      }),
      (pattern) => turnEventMap(pattern),
      (entries) => {
        expect(entries.map(e => e.tool_name)).toEqual(["Grep", "Write", "Bash"]);
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// Feature: agentSessionTurns — resolve subagent turns by session ID
// ═══════════════════════════════════════════════════════════════════

describe("agentSessionTurns", () => {
  const allPatterns: PatternView[] = [
    makeSentenceWithOutcomes({ turn: 1, session_id: "main-session" }),
    makeSentenceWithOutcomes({ turn: 2, session_id: "main-session" }),
    makeSentenceWithOutcomes({ turn: 0, session_id: "agent-abc123" }),
    makeSentenceWithOutcomes({ turn: 1, session_id: "agent-abc123" }),
    makeSentenceWithOutcomes({ turn: 0, session_id: "agent-def456" }),
  ];

  it("should return turns for a matching agent session", () => {
    scenario(
      () => ({ id: "agent-abc123", patterns: allPatterns }),
      ({ id, patterns }) => agentSessionTurns(id, patterns),
      (turns) => {
        expect(turns).toHaveLength(2);
        expect((turns[0]!.metadata as any).turn).toBe(0);
        expect((turns[1]!.metadata as any).turn).toBe(1);
      },
    );
  });

  it("should return empty array for unknown session", () => {
    scenario(
      () => ({ id: "agent-unknown", patterns: allPatterns }),
      ({ id, patterns }) => agentSessionTurns(id, patterns),
      (turns) => expect(turns).toHaveLength(0),
    );
  });

  it("should sort by turn number", () => {
    const unordered: PatternView[] = [
      makeSentenceWithOutcomes({ turn: 2, session_id: "agent-x" }),
      makeSentenceWithOutcomes({ turn: 0, session_id: "agent-x" }),
      makeSentenceWithOutcomes({ turn: 1, session_id: "agent-x" }),
    ];
    scenario(
      () => ({ id: "agent-x", patterns: unordered }),
      ({ id, patterns }) => agentSessionTurns(id, patterns),
      (turns) => {
        const turnNums = turns.map(t => (t.metadata as any).turn);
        expect(turnNums).toEqual([0, 1, 2]);
      },
    );
  });
});
