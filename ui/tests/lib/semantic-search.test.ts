import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  formatScore,
  truncateSnippet,
  recordTypeLabel,
  groupBySession,
  type SemanticSearchResult,
} from "@/lib/semantic-search";

// ── formatScore — boundary table ──────────────────────

const SCORE_TABLE: [string, number, string][] = [
  ["perfect match", 1.0, "100%"],
  ["high relevance", 0.95, "95%"],
  ["mid relevance", 0.5, "50%"],
  ["low relevance", 0.12, "12%"],
  ["zero", 0.0, "0%"],
  ["rounds down", 0.944, "94%"],
  ["rounds up", 0.945, "95%"],
];

describe("formatScore — boundary table", () => {
  it.each(SCORE_TABLE)("%s: %f → %s", (_desc, score, expected) => {
    scenario(
      () => score,
      (s) => formatScore(s),
      (result) => expect(result).toBe(expected),
    );
  });
});

// ── truncateSnippet — boundary table ──────────────────────

const TRUNCATE_TABLE: [string, string, number, string][] = [
  ["short text unchanged", "hello", 10, "hello"],
  ["exact length unchanged", "hello", 5, "hello"],
  ["truncated with ellipsis", "hello world foo", 11, "hello world…"],
  ["empty string", "", 10, ""],
  ["one char limit", "hello", 1, "h…"],
];

describe("truncateSnippet — boundary table", () => {
  it.each(TRUNCATE_TABLE)("%s", (_desc, text, maxLen, expected) => {
    scenario(
      () => ({ text, maxLen }),
      ({ text, maxLen }) => truncateSnippet(text, maxLen),
      (result) => expect(result).toBe(expected),
    );
  });
});

// ── recordTypeLabel ──────────────────────

describe("recordTypeLabel", () => {
  it("maps known types", () => {
    expect(recordTypeLabel("user_message")).toBe("User");
    expect(recordTypeLabel("assistant_message")).toBe("Assistant");
    expect(recordTypeLabel("tool_call")).toBe("Tool Call");
    expect(recordTypeLabel("tool_result")).toBe("Tool Result");
    expect(recordTypeLabel("reasoning")).toBe("Thinking");
    expect(recordTypeLabel("error")).toBe("Error");
  });

  it("returns raw type for unknown", () => {
    expect(recordTypeLabel("custom_thing")).toBe("custom_thing");
  });
});

// ── groupBySession ──────────────────────

function makeResult(overrides: Partial<SemanticSearchResult> = {}): SemanticSearchResult {
  return {
    event_id: "evt-1",
    session_id: "sess-1",
    score: 0.9,
    text_snippet: "test snippet",
    metadata: {
      record_type: "user_message",
      timestamp: "2025-01-17T00:00:00Z",
    },
    ...overrides,
  };
}

describe("groupBySession", () => {
  it("groups results by session_id", () => {
    const results = [
      makeResult({ event_id: "e1", session_id: "s1", score: 0.9 }),
      makeResult({ event_id: "e2", session_id: "s2", score: 0.8 }),
      makeResult({ event_id: "e3", session_id: "s1", score: 0.7 }),
    ];
    scenario(
      () => ({ results, limit: 10 }),
      ({ results, limit }) => groupBySession(results, limit),
      (groups) => {
        expect(groups).toHaveLength(2);
        expect(groups[0]!.session_id).toBe("s1"); // higher max score
        expect(groups[0]!.relevance_score).toBe(0.9);
        expect(groups[0]!.matching_events).toHaveLength(2);
        expect(groups[1]!.session_id).toBe("s2");
      },
    );
  });

  it("limits to top N sessions", () => {
    const results = [
      makeResult({ session_id: "s1", score: 0.9 }),
      makeResult({ session_id: "s2", score: 0.8 }),
      makeResult({ session_id: "s3", score: 0.7 }),
    ];
    scenario(
      () => ({ results, limit: 2 }),
      ({ results, limit }) => groupBySession(results, limit),
      (groups) => expect(groups).toHaveLength(2),
    );
  });

  it("sorts by max score descending", () => {
    const results = [
      makeResult({ session_id: "s1", score: 0.5 }),
      makeResult({ session_id: "s2", score: 0.9 }),
      makeResult({ session_id: "s3", score: 0.7 }),
    ];
    scenario(
      () => ({ results, limit: 10 }),
      ({ results, limit }) => groupBySession(results, limit),
      (groups) => {
        expect(groups[0]!.session_id).toBe("s2");
        expect(groups[1]!.session_id).toBe("s3");
        expect(groups[2]!.session_id).toBe("s1");
      },
    );
  });

  it("takes top 3 events per session", () => {
    const results = Array.from({ length: 5 }, (_, i) =>
      makeResult({ event_id: `e${i}`, session_id: "s1", score: 0.9 - i * 0.1 }),
    );
    scenario(
      () => ({ results, limit: 10 }),
      ({ results, limit }) => groupBySession(results, limit),
      (groups) => {
        expect(groups[0]!.matching_events).toHaveLength(3);
      },
    );
  });

  it("empty input returns empty", () => {
    scenario(
      () => ({ results: [] as SemanticSearchResult[], limit: 10 }),
      ({ results, limit }) => groupBySession(results, limit),
      (groups) => expect(groups).toHaveLength(0),
    );
  });

  it("generates synopsis_url and tool_journey_url", () => {
    const results = [makeResult({ session_id: "abc-123" })];
    scenario(
      () => ({ results, limit: 10 }),
      ({ results, limit }) => groupBySession(results, limit),
      (groups) => {
        expect(groups[0]!.synopsis_url).toBe("/api/sessions/abc-123/synopsis");
        expect(groups[0]!.tool_journey_url).toBe("/api/sessions/abc-123/tool-journey");
      },
    );
  });
});
