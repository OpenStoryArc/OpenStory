/**
 * Spec: Story view feature functions — pure data transformations.
 *
 * Each test covers a feature from the Live tab translated to Story:
 *   - Category filtering (pure_text, tool_use, thinking, delegation, error)
 *   - Session grouping and sorting
 *   - Turn phase extraction from sentence metadata
 *   - Scope depth profiling
 *   - Turn-in-progress detection
 *   - Cross-linking (turn → event IDs)
 *
 * These are pure function tests — no React rendering, no DOM.
 * The functions will live in ui/src/lib/story.ts.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import type { PatternView } from "@/types/wire-record";

// ═══════════════════════════════════════════════════════════════════
// Test data factories
// ═══════════════════════════════════════════════════════════════════

function makeSentence(overrides: Partial<{
  turn: number;
  session_id: string;
  verb: string;
  predicate: string;
  is_terminal: boolean;
  stop_reason: string;
  scope_depth: number;
  env_size: number;
  env_delta: number;
  decision: string;
  human_content: string;
  eval_content: string;
  applies: Array<{ tool_name: string; is_error: boolean; is_agent: boolean }>;
  thinking_summary: string;
}> = {}): PatternView {
  const turn = overrides.turn ?? 1;
  const session_id = overrides.session_id ?? "sess-1";
  const verb = overrides.verb ?? "checked";
  const predicate = overrides.predicate ?? "answered";

  return {
    type: "turn.sentence",
    label: `Claude ${verb} something → ${predicate}`,
    session_id,
    events: [`evt-${turn}-1`, `evt-${turn}-2`],
    metadata: {
      turn,
      subject: "Claude",
      verb,
      object: "something",
      adverbial: null,
      predicate,
      subordinates: [],
      scope_depth: overrides.scope_depth ?? 0,
      human: overrides.human_content
        ? { content: overrides.human_content, timestamp: "2026-01-01T00:00:00Z" }
        : null,
      thinking: overrides.thinking_summary
        ? { summary: overrides.thinking_summary }
        : null,
      eval: overrides.eval_content
        ? { content: overrides.eval_content, decision: overrides.decision ?? "text_only", timestamp: "2026-01-01T00:00:01Z" }
        : { content: "response", decision: overrides.decision ?? "text_only", timestamp: "2026-01-01T00:00:01Z" },
      applies: (overrides.applies ?? []).map(a => ({
        tool_name: a.tool_name,
        input_summary: "",
        output_summary: "",
        is_error: a.is_error,
        is_agent: a.is_agent,
        tool_outcome: a.tool_name === "Write" ? { type: "FileCreated", path: "/test.ts" }
          : a.tool_name === "Edit" ? { type: "FileModified", path: "/test.ts" }
          : a.tool_name === "Read" ? { type: "FileRead", path: "/test.ts" }
          : a.tool_name === "Bash" ? { type: "CommandExecuted", command: "npm test", succeeded: !a.is_error }
          : a.tool_name === "Grep" || a.tool_name === "Glob" ? { type: "SearchPerformed", pattern: "*.ts", source: "filesystem" }
          : a.tool_name === "Agent" ? { type: "SubAgentSpawned", description: "explore" }
          : null,
      })),
      env_size: overrides.env_size ?? 5,
      env_delta: overrides.env_delta ?? 2,
      stop_reason: overrides.stop_reason ?? "end_turn",
      is_terminal: overrides.is_terminal ?? true,
      duration_ms: null,
    },
  };
}

// ═══════════════════════════════════════════════════════════════════
// Feature: Category filtering
// ═══════════════════════════════════════════════════════════════════
//
// Live has: prompt, response, thinking, tool, result, system, error, turn
// Story has: pure_text, tool_use, thinking, delegation, error

import {
  filterSentences,
  categorizeTurn,
} from "@/lib/story";

describe("categorizeTurn", () => {
  it("should classify turn with no tools as pure_text", () => {
    scenario(
      () => makeSentence({ decision: "text_only", applies: [] }),
      (s) => categorizeTurn(s),
      (cat) => expect(cat).toBe("pure_text"),
    );
  });

  it("should classify turn with tools as tool_use", () => {
    scenario(
      () => makeSentence({ decision: "tool_use", applies: [{ tool_name: "Bash", is_error: false, is_agent: false }] }),
      (s) => categorizeTurn(s),
      (cat) => expect(cat).toBe("tool_use"),
    );
  });

  it("should classify turn with thinking as thinking", () => {
    scenario(
      () => makeSentence({ thinking_summary: "Let me reason about this..." }),
      (s) => categorizeTurn(s),
      (cat) => expect(cat).toBe("thinking"),
    );
  });

  it("should classify turn with Agent tool as delegation", () => {
    scenario(
      () => makeSentence({ applies: [{ tool_name: "Agent", is_error: false, is_agent: true }] }),
      (s) => categorizeTurn(s),
      (cat) => expect(cat).toBe("delegation"),
    );
  });

  it("should classify turn with errors as error", () => {
    scenario(
      () => makeSentence({ applies: [{ tool_name: "Bash", is_error: true, is_agent: false }] }),
      (s) => categorizeTurn(s),
      (cat) => expect(cat).toBe("error"),
    );
  });
});

describe("filterSentences", () => {
  it("should return all sentences when no filter applied", () => {
    const sentences = [
      makeSentence({ turn: 1, decision: "text_only" }),
      makeSentence({ turn: 2, decision: "tool_use", applies: [{ tool_name: "Bash", is_error: false, is_agent: false }] }),
    ];
    expect(filterSentences(sentences, new Set())).toHaveLength(2);
  });

  it("should filter to only pure_text turns", () => {
    const sentences = [
      makeSentence({ turn: 1, decision: "text_only" }),
      makeSentence({ turn: 2, decision: "tool_use", applies: [{ tool_name: "Bash", is_error: false, is_agent: false }] }),
      makeSentence({ turn: 3, decision: "text_only" }),
    ];
    const filtered = filterSentences(sentences, new Set(["pure_text"]));
    expect(filtered).toHaveLength(2);
  });

  it("should filter to only error turns", () => {
    const sentences = [
      makeSentence({ turn: 1 }),
      makeSentence({ turn: 2, applies: [{ tool_name: "Bash", is_error: true, is_agent: false }] }),
      makeSentence({ turn: 3 }),
    ];
    const filtered = filterSentences(sentences, new Set(["error"]));
    expect(filtered).toHaveLength(1);
  });
});

// ═══════════════════════════════════════════════════════════════════
// Feature: Scope depth profiling
// ═══════════════════════════════════════════════════════════════════

import { scopeDepthProfile } from "@/lib/story";

describe("scopeDepthProfile", () => {
  it("should extract scope_depth from each sentence", () => {
    const sentences = [
      makeSentence({ turn: 1, scope_depth: 0 }),
      makeSentence({ turn: 2, scope_depth: 1 }),
      makeSentence({ turn: 3, scope_depth: 2 }),
      makeSentence({ turn: 4, scope_depth: 0 }),
    ];
    expect(scopeDepthProfile(sentences)).toEqual([0, 1, 2, 0]);
  });

  it("should return empty array for no sentences", () => {
    expect(scopeDepthProfile([])).toEqual([]);
  });
});

// ═══════════════════════════════════════════════════════════════════
// Feature: Session sorting and grouping
// ═══════════════════════════════════════════════════════════════════

import { groupBySession } from "@/lib/story";

describe("groupBySession", () => {
  it("should group sentences by session_id", () => {
    const sentences = [
      makeSentence({ turn: 1, session_id: "s1" }),
      makeSentence({ turn: 2, session_id: "s2" }),
      makeSentence({ turn: 3, session_id: "s1" }),
    ];
    const groups = groupBySession(sentences);
    expect(groups.get("s1")).toHaveLength(2);
    expect(groups.get("s2")).toHaveLength(1);
  });

  it("should return empty map for no sentences", () => {
    expect(groupBySession([]).size).toBe(0);
  });
});

// ═══════════════════════════════════════════════════════════════════
// Feature: Story phase bar (verb distribution)
// ═══════════════════════════════════════════════════════════════════

import { verbDistribution } from "@/lib/story";

describe("verbDistribution", () => {
  it("should count occurrences of each verb", () => {
    const sentences = [
      makeSentence({ verb: "wrote" }),
      makeSentence({ verb: "wrote" }),
      makeSentence({ verb: "read" }),
      makeSentence({ verb: "explained" }),
    ];
    const dist = verbDistribution(sentences);
    expect(dist.get("wrote")).toBe(2);
    expect(dist.get("read")).toBe(1);
    expect(dist.get("explained")).toBe(1);
  });
});

// ═══════════════════════════════════════════════════════════════════
// Feature: Cross-linking (turn → event IDs)
// ═══════════════════════════════════════════════════════════════════

import { turnEventIds } from "@/lib/story";

describe("turnEventIds", () => {
  it("should return the event_ids from a sentence pattern", () => {
    const s = makeSentence({ turn: 5 });
    expect(turnEventIds(s)).toEqual(["evt-5-1", "evt-5-2"]);
  });
});

// ═══════════════════════════════════════════════════════════════════
// Feature: Turn-in-progress detection
// ═══════════════════════════════════════════════════════════════════

import { isInProgress } from "@/lib/story";

describe("isInProgress", () => {
  it("should return false for terminal turns", () => {
    expect(isInProgress(makeSentence({ is_terminal: true }))).toBe(false);
  });

  it("should return false for continued turns (they completed, just not terminal)", () => {
    expect(isInProgress(makeSentence({ is_terminal: false }))).toBe(false);
  });
});

// ═══════════════════════════════════════════════════════════════════
// Feature: Environment growth tracking
// ═══════════════════════════════════════════════════════════════════

import { envGrowthSeries } from "@/lib/story";

describe("envGrowthSeries", () => {
  it("should extract env_size from each sentence as a series", () => {
    const sentences = [
      makeSentence({ turn: 1, env_size: 3 }),
      makeSentence({ turn: 2, env_size: 8 }),
      makeSentence({ turn: 3, env_size: 15 }),
    ];
    expect(envGrowthSeries(sentences)).toEqual([3, 8, 15]);
  });
});
