/**
 * Spec: story-api — REST data fetching and merge logic.
 *
 * Tests the pure merge function that deduplicates WebSocket
 * patterns against cached REST data. Fetch functions are
 * integration-tested via the E2E suite.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildSessionsQuery, mergeSentences } from "@/lib/story-api";
import type { PatternView } from "@/types/wire-record";

// ═══════════════════════════════════════════════════════════════════
// Factory
// ═══════════════════════════════════════════════════════════════════

function makeSentence(
  sessionId: string,
  turn: number,
  verb: string = "checked",
): PatternView {
  return {
    type: "turn.sentence",
    label: `Claude ${verb} something`,
    session_id: sessionId,
    events: [`evt-${turn}`],
    metadata: {
      turn,
      verb,
      subject: "Claude",
      object: "something",
      is_terminal: true,
    },
  };
}

// ═══════════════════════════════════════════════════════════════════
// mergeSentences — dedup WebSocket patterns against cache
// ═══════════════════════════════════════════════════════════════════

describe("mergeSentences", () => {
  it("should return null when no new sentences", () => {
    scenario(
      () => ({
        existing: [makeSentence("s1", 1), makeSentence("s1", 2)],
        incoming: [makeSentence("s1", 1), makeSentence("s1", 2)],
      }),
      ({ existing, incoming }) => mergeSentences(existing, incoming),
      (result) => expect(result).toBeNull(),
    );
  });

  it("should append new sentences", () => {
    scenario(
      () => ({
        existing: [makeSentence("s1", 1)],
        incoming: [makeSentence("s1", 2), makeSentence("s1", 3)],
      }),
      ({ existing, incoming }) => mergeSentences(existing, incoming),
      (result) => {
        expect(result).not.toBeNull();
        expect(result).toHaveLength(3);
        expect(result![2]!.metadata!.turn).toBe(3);
      },
    );
  });

  it("should filter non-sentence patterns from incoming", () => {
    scenario(
      () => ({
        existing: [makeSentence("s1", 1)],
        incoming: [
          { ...makeSentence("s1", 2), type: "eval_apply" } as PatternView,
          makeSentence("s1", 3),
        ],
      }),
      ({ existing, incoming }) => mergeSentences(existing, incoming),
      (result) => {
        expect(result).not.toBeNull();
        expect(result).toHaveLength(2); // existing + turn 3 only
      },
    );
  });

  it("should dedup by session_id + turn number", () => {
    scenario(
      () => ({
        existing: [makeSentence("s1", 1), makeSentence("s2", 1)],
        incoming: [
          makeSentence("s1", 1), // dup
          makeSentence("s2", 1), // dup
          makeSentence("s1", 2), // new
        ],
      }),
      ({ existing, incoming }) => mergeSentences(existing, incoming),
      (result) => {
        expect(result).not.toBeNull();
        expect(result).toHaveLength(3);
      },
    );
  });

  it("should merge into empty cache", () => {
    scenario(
      () => ({
        existing: [] as PatternView[],
        incoming: [makeSentence("s1", 1)],
      }),
      ({ existing, incoming }) => mergeSentences(existing, incoming),
      (result) => {
        expect(result).not.toBeNull();
        expect(result).toHaveLength(1);
      },
    );
  });

  it("should return null for empty incoming", () => {
    scenario(
      () => ({
        existing: [makeSentence("s1", 1)],
        incoming: [] as PatternView[],
      }),
      ({ existing, incoming }) => mergeSentences(existing, incoming),
      (result) => expect(result).toBeNull(),
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// buildSessionsQuery — query string builder for /api/sessions
// ═══════════════════════════════════════════════════════════════════

describe("buildSessionsQuery", () => {
  it("emits only limit when no sort or since", () => {
    expect(buildSessionsQuery({ limit: 10 })).toBe("limit=10");
  });

  it("defaults limit to 5 when omitted", () => {
    expect(buildSessionsQuery({})).toBe("limit=5");
  });

  it("omits sort when latest (server default)", () => {
    expect(buildSessionsQuery({ limit: 5, sort: "latest" })).toBe("limit=5");
  });

  it("includes non-default sort", () => {
    expect(buildSessionsQuery({ limit: 5, sort: "active" })).toBe(
      "limit=5&sort=active",
    );
    expect(buildSessionsQuery({ limit: 5, sort: "tokens" })).toBe(
      "limit=5&sort=tokens",
    );
  });

  it("includes since when provided", () => {
    expect(
      buildSessionsQuery({ limit: 5, since: "2026-04-30T00:00:00Z" }),
    ).toBe("limit=5&since=2026-04-30T00%3A00%3A00Z");
  });

  it("combines sort and since", () => {
    expect(
      buildSessionsQuery({
        limit: 20,
        sort: "tokens",
        since: "2026-04-23T00:00:00Z",
      }),
    ).toBe("limit=20&sort=tokens&since=2026-04-23T00%3A00%3A00Z");
  });
});
