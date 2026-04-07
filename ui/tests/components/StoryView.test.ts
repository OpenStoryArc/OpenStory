/**
 * Spec: StoryView feature wiring — pure data derivations.
 *
 * Tests the data transformations that feed the StoryView component:
 * category filtering, stats bar data, verb distribution, env growth,
 * keyboard navigation index, and cross-link URL building.
 *
 * These test the WIRING — that the pure functions from lib/story.ts
 * are correctly composed to produce the data the UI needs.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import type { PatternView } from "@/types/wire-record";
import {
  categorizeTurn,
  filterSentences,
  verbDistribution,
  scopeDepthProfile,
  envGrowthSeries,
  groupBySession,
  type StoryCategory,
} from "@/lib/story";

// ═══════════════════════════════════════════════════════════════════
// Factory
// ═══════════════════════════════════════════════════════════════════

function makeSentence(overrides: {
  turn?: number;
  session_id?: string;
  verb?: string;
  is_terminal?: boolean;
  scope_depth?: number;
  env_size?: number;
  decision?: string;
  applies?: Array<{ tool_name: string; is_error: boolean; is_agent: boolean }>;
  thinking?: string;
} = {}): PatternView {
  const turn = overrides.turn ?? 1;
  return {
    type: "turn.sentence",
    label: `Claude ${overrides.verb ?? "checked"} something → ${overrides.is_terminal === false ? "continued" : "answered"}`,
    session_id: overrides.session_id ?? "sess-1",
    events: [`evt-${turn}`],
    metadata: {
      turn,
      verb: overrides.verb ?? "checked",
      subject: "Claude",
      object: "something",
      predicate: overrides.is_terminal === false ? "continued" : "answered",
      subordinates: [],
      adverbial: null,
      scope_depth: overrides.scope_depth ?? 0,
      env_size: overrides.env_size ?? 5,
      env_delta: 2,
      stop_reason: overrides.is_terminal === false ? "tool_use" : "end_turn",
      is_terminal: overrides.is_terminal ?? true,
      duration_ms: null,
      human: null,
      thinking: overrides.thinking ? { summary: overrides.thinking } : null,
      eval: { content: "response", decision: overrides.decision ?? "text_only", timestamp: "" },
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
    },
  };
}

// ═══════════════════════════════════════════════════════════════════
// Stats bar: verb distribution for display
// ═══════════════════════════════════════════════════════════════════

describe("stats bar data derivation", () => {
  it("should compute verb distribution from sentences", () => {
    scenario(
      () => [
        makeSentence({ verb: "wrote" }),
        makeSentence({ verb: "wrote" }),
        makeSentence({ verb: "read" }),
        makeSentence({ verb: "delegated" }),
        makeSentence({ verb: "wrote" }),
      ],
      (sentences) => verbDistribution(sentences),
      (dist) => {
        expect(dist.get("wrote")).toBe(3);
        expect(dist.get("read")).toBe(1);
        expect(dist.get("delegated")).toBe(1);
      },
    );
  });

  it("should compute env growth series for sparkline", () => {
    scenario(
      () => [
        makeSentence({ turn: 1, env_size: 3 }),
        makeSentence({ turn: 2, env_size: 8 }),
        makeSentence({ turn: 3, env_size: 20 }),
      ],
      (sentences) => envGrowthSeries(sentences),
      (series) => expect(series).toEqual([3, 8, 20]),
    );
  });

  it("should compute scope depth profile for sparkline", () => {
    scenario(
      () => [
        makeSentence({ scope_depth: 0 }),
        makeSentence({ scope_depth: 1 }),
        makeSentence({ scope_depth: 2 }),
        makeSentence({ scope_depth: 0 }),
      ],
      (sentences) => scopeDepthProfile(sentences),
      (profile) => expect(profile).toEqual([0, 1, 2, 0]),
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// Category filter: toggle categories on/off
// ═══════════════════════════════════════════════════════════════════

describe("category filter composition", () => {
  const mixedSentences = [
    makeSentence({ turn: 1, verb: "explained", decision: "text_only" }),
    makeSentence({ turn: 2, verb: "wrote", decision: "tool_use", applies: [{ tool_name: "Write", is_error: false, is_agent: false }] }),
    makeSentence({ turn: 3, verb: "delegated", applies: [{ tool_name: "Agent", is_error: false, is_agent: true }] }),
    makeSentence({ turn: 4, verb: "checked", applies: [{ tool_name: "Bash", is_error: true, is_agent: false }] }),
    makeSentence({ turn: 5, verb: "explained", thinking: "Let me think...", decision: "text_only" }),
  ];

  it("should show all when no filter", () => {
    expect(filterSentences(mixedSentences, new Set())).toHaveLength(5);
  });

  it("should filter to conversation only (pure_text)", () => {
    const filtered = filterSentences(mixedSentences, new Set(["pure_text"] as StoryCategory[]));
    expect(filtered).toHaveLength(1);
    expect(filtered[0]!.metadata!.turn).toBe(1);
  });

  it("should filter to errors only", () => {
    const filtered = filterSentences(mixedSentences, new Set(["error"] as StoryCategory[]));
    expect(filtered).toHaveLength(1);
    expect(filtered[0]!.metadata!.turn).toBe(4);
  });

  it("should combine multiple categories", () => {
    const filtered = filterSentences(mixedSentences, new Set(["pure_text", "thinking"] as StoryCategory[]));
    expect(filtered).toHaveLength(2);
  });

  it("should filter delegation turns", () => {
    const filtered = filterSentences(mixedSentences, new Set(["delegation"] as StoryCategory[]));
    expect(filtered).toHaveLength(1);
    expect(filtered[0]!.metadata!.verb).toBe("delegated");
  });
});

// ═══════════════════════════════════════════════════════════════════
// Session grouping for sidebar
// ═══════════════════════════════════════════════════════════════════

describe("session grouping for sidebar", () => {
  it("should group by session and count turns", () => {
    const sentences = [
      makeSentence({ session_id: "s1", turn: 1 }),
      makeSentence({ session_id: "s1", turn: 2 }),
      makeSentence({ session_id: "s1", turn: 3 }),
      makeSentence({ session_id: "s2", turn: 1 }),
    ];
    const groups = groupBySession(sentences);
    expect(groups.get("s1")).toHaveLength(3);
    expect(groups.get("s2")).toHaveLength(1);
  });

  it("should derive category counts per session", () => {
    const sentences = [
      makeSentence({ session_id: "s1", decision: "text_only" }),
      makeSentence({ session_id: "s1", decision: "tool_use", applies: [{ tool_name: "Bash", is_error: false, is_agent: false }] }),
      makeSentence({ session_id: "s1", applies: [{ tool_name: "Bash", is_error: true, is_agent: false }] }),
    ];
    const categories = sentences.map(categorizeTurn);
    expect(categories).toEqual(["pure_text", "tool_use", "error"]);
  });
});

// ═══════════════════════════════════════════════════════════════════
// Terminal vs continue ratio
// ═══════════════════════════════════════════════════════════════════

describe("terminal vs continue stats", () => {
  it("should count terminal and continued turns", () => {
    const sentences = [
      makeSentence({ is_terminal: true }),
      makeSentence({ is_terminal: false }),
      makeSentence({ is_terminal: false }),
      makeSentence({ is_terminal: true }),
    ];
    const terminal = sentences.filter(s => (s.metadata as Record<string, unknown>).is_terminal === true).length;
    const continued = sentences.filter(s => (s.metadata as Record<string, unknown>).is_terminal === false).length;
    expect(terminal).toBe(2);
    expect(continued).toBe(2);
  });
});
