import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { computeExploreCounts, applyExploreFilter, filterNoise } from "@/lib/explore-filters";
import type { WireRecord } from "@/types/wire-record";

function makeRecord(overrides: Partial<WireRecord>): WireRecord {
  return {
    id: "test-id",
    seq: 1,
    session_id: "s1",
    timestamp: "2026-01-01T00:00:00Z",
    record_type: "assistant_message",
    payload: { text: "hello" },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 100,
    ...overrides,
  } as WireRecord;
}

// ── computeExploreCounts ──────────────────────

describe("computeExploreCounts", () => {
  it("returns zero counts for empty records", () => {
    scenario(
      () => [] as WireRecord[],
      (records) => computeExploreCounts(records),
      (counts) => {
        expect(counts["conversation"]).toBe(0);
        expect(counts["errors"]).toBe(0);
        expect(counts["thinking"]).toBe(0);
      },
    );
  });

  it("counts conversation messages", () => {
    const records = [
      makeRecord({ id: "1", record_type: "user_message" }),
      makeRecord({ id: "2", record_type: "assistant_message" }),
      makeRecord({ id: "3", record_type: "tool_call", payload: { name: "Read", raw_input: {} } }),
    ];

    scenario(
      () => records,
      (r) => computeExploreCounts(r),
      (counts) => {
        expect(counts["conversation"]).toBe(2);
      },
    );
  });

  it("counts errors", () => {
    const records = [
      makeRecord({ id: "1", record_type: "error" }),
      makeRecord({ id: "2", record_type: "assistant_message" }),
    ];

    scenario(
      () => records,
      (r) => computeExploreCounts(r),
      (counts) => {
        expect(counts["errors"]).toBe(1);
      },
    );
  });

  it("does not include 'all' in counts", () => {
    scenario(
      () => [makeRecord({ id: "1" })],
      (r) => computeExploreCounts(r),
      (counts) => {
        expect(counts["all"]).toBeUndefined();
      },
    );
  });
});

// ── applyExploreFilter ──────────────────────

describe("applyExploreFilter", () => {
  const records = [
    makeRecord({ id: "1", record_type: "user_message" }),
    makeRecord({ id: "2", record_type: "assistant_message" }),
    makeRecord({ id: "3", record_type: "error" }),
    makeRecord({ id: "4", record_type: "reasoning" }),
  ];

  it("'all' returns everything", () => {
    scenario(
      () => ({ records, filter: "all" }),
      ({ records, filter }) => applyExploreFilter(records, filter),
      (result) => expect(result).toHaveLength(4),
    );
  });

  it("'conversation' returns user + assistant messages", () => {
    scenario(
      () => ({ records, filter: "conversation" }),
      ({ records, filter }) => applyExploreFilter(records, filter),
      (result) => {
        expect(result).toHaveLength(2);
        expect(result.map((r) => r.record_type)).toEqual(["user_message", "assistant_message"]);
      },
    );
  });

  it("'errors' returns error records", () => {
    scenario(
      () => ({ records, filter: "errors" }),
      ({ records, filter }) => applyExploreFilter(records, filter),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.record_type).toBe("error");
      },
    );
  });

  it("'thinking' returns reasoning records", () => {
    scenario(
      () => ({ records, filter: "thinking" }),
      ({ records, filter }) => applyExploreFilter(records, filter),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.record_type).toBe("reasoning");
      },
    );
  });

  it("unknown filter returns everything", () => {
    scenario(
      () => ({ records, filter: "nonexistent" }),
      ({ records, filter }) => applyExploreFilter(records, filter),
      (result) => expect(result).toHaveLength(4),
    );
  });
});

// ── filterNoise ──────────────────────

describe("filterNoise", () => {
  it("removes token_usage, file_snapshot, session_meta, context_compaction", () => {
    const records = [
      makeRecord({ id: "1", record_type: "user_message" }),
      makeRecord({ id: "2", record_type: "token_usage" }),
      makeRecord({ id: "3", record_type: "tool_call", payload: { name: "Read", raw_input: {} } }),
      makeRecord({ id: "4", record_type: "file_snapshot" }),
      makeRecord({ id: "5", record_type: "session_meta" }),
      makeRecord({ id: "6", record_type: "assistant_message" }),
      makeRecord({ id: "7", record_type: "context_compaction" }),
    ];

    scenario(
      () => records,
      (r) => filterNoise(r),
      (result) => {
        expect(result).toHaveLength(3);
        expect(result.map((r) => r.record_type)).toEqual([
          "user_message", "tool_call", "assistant_message",
        ]);
      },
    );
  });

  it("keeps everything when no noise", () => {
    const records = [
      makeRecord({ id: "1", record_type: "tool_call", payload: { name: "Bash", raw_input: {} } }),
      makeRecord({ id: "2", record_type: "tool_result" }),
    ];

    scenario(
      () => records,
      (r) => filterNoise(r),
      (result) => expect(result).toHaveLength(2),
    );
  });

  it("empty input returns empty", () => {
    scenario(
      () => [] as WireRecord[],
      (r) => filterNoise(r),
      (result) => expect(result).toHaveLength(0),
    );
  });
});
