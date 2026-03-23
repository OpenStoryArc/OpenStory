/**
 * Truncation pipeline tests — verifies behavior from server payload through rendering.
 *
 * Pipeline:
 *   Server (truncation_threshold=2000) → WireRecord (truncated flag, payload_bytes)
 *     → toTimelineRows (MAX_SUMMARY=500) → TimelineRow.summary (truncated)
 *       → CardBody reads record.payload (full text, bypasses summary)
 *
 * These tests verify data availability at each stage.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { toTimelineRows } from "@/lib/timeline";
import type { WireRecord } from "@/types/wire-record";

function makeRecord(id: string, overrides: Omit<Partial<WireRecord>, "payload"> & { payload?: unknown } = {}): WireRecord {
  return {
    id,
    seq: 1,
    session_id: "s1",
    timestamp: "2026-01-01T00:00:00Z",
    record_type: "assistant_message",
    payload: { model: "test", content: [{ type: "text", text: "test" }] },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 100,
    ...overrides,
  } as unknown as WireRecord;
}

// ── Stage 1: toTimelineRows summary truncation ──────────────────────

/** Build a content-blocks payload matching the real UserMessage/AssistantMessage shape. */
function blocksPayload(text: string) {
  return { content: [{ type: "text", text }] };
}

const SUMMARY_TRUNCATION_TABLE: [string, string, number, boolean, (t: string) => any][] = [
  // description, record_type, text_length, expect_truncated, payload_builder
  ["short prompt passes through",     "user_message",      50,  false, blocksPayload],
  ["long prompt truncated to 500",    "user_message",      600, true,  blocksPayload],
  ["short response passes through",   "assistant_message",  50,  false, blocksPayload],
  ["long response truncated to 500",  "assistant_message",  800, true,  blocksPayload],
  ["short result passes through",     "tool_result",        50,  false, (t: string) => ({ output: t, is_error: false })],
  ["long result truncated to 500",    "tool_result",        800, true,  (t: string) => ({ output: t, is_error: false })],
];

describe("Stage 1: toTimelineRows summary truncation — boundary table", () => {
  it.each(SUMMARY_TRUNCATION_TABLE)(
    "%s",
    (_desc, recordType, textLength, expectTruncated, buildPayload) => {
      const text = "x".repeat(textLength);
      const payload = buildPayload(text);
      const records = [makeRecord("1", { record_type: recordType as any, payload })];

      scenario(
        () => records,
        (r) => {
          const rows = toTimelineRows(r);
          const row = rows.find((r) => r.category !== "turn");
          return { summaryLength: row!.summary.length, isTruncated: row!.summary.length < textLength };
        },
        ({ summaryLength, isTruncated }) => {
          if (expectTruncated) {
            expect(summaryLength).toBeLessThanOrEqual(500);
            expect(isTruncated).toBe(true);
          } else {
            expect(summaryLength).toBe(textLength);
            expect(isTruncated).toBe(false);
          }
        },
      );
    },
  );
});

// ── Stage 2: Full text preserved on record payload ──────────────────────

describe("Stage 2: full text preserved on record.payload despite summary truncation", () => {
  it("user_message payload content blocks are untruncated", () => {
    const text = "x".repeat(600);
    const records = [makeRecord("1", { record_type: "user_message", payload: blocksPayload(text) })];

    scenario(
      () => records,
      (r) => toTimelineRows(r),
      (rows) => {
        const row = rows.find((r) => r.category === "prompt")!;
        expect(row.summary.length).toBeLessThanOrEqual(500); // summary truncated
        const blocks = (row.record.payload as any).content as { text: string }[];
        expect(blocks[0]!.text.length).toBe(600); // payload full
      },
    );
  });

  it("assistant_message payload content blocks are untruncated", () => {
    const text = "z".repeat(1000);
    const records = [makeRecord("1", { record_type: "assistant_message", payload: blocksPayload(text) })];

    scenario(
      () => records,
      (r) => toTimelineRows(r),
      (rows) => {
        const row = rows.find((r) => r.category === "response")!;
        expect(row.summary.length).toBeLessThanOrEqual(500);
        const blocks = (row.record.payload as any).content as { text: string }[];
        expect(blocks[0]!.text.length).toBe(1000); // payload full
      },
    );
  });

  it("tool_result payload.output is untruncated", () => {
    const output = "y".repeat(800);
    const records = [makeRecord("1", { record_type: "tool_result", payload: { output, is_error: false } })];

    scenario(
      () => records,
      (r) => toTimelineRows(r),
      (rows) => {
        const row = rows.find((r) => r.category === "result")!;
        expect(row.summary.length).toBeLessThanOrEqual(500);
        expect(((row.record.payload as any).output as string).length).toBe(800);
      },
    );
  });
});

// ── Stage 3: Server-side truncation metadata ──────────────────────

describe("Stage 3: server-side truncation — WireRecord metadata", () => {
  it("truncated=true when server capped the payload", () => {
    const record = makeRecord("1", {
      record_type: "tool_result",
      payload: { output: "x".repeat(2000) },
      truncated: true,
      payload_bytes: 43741,
    });

    scenario(
      () => record,
      (r) => ({ truncated: r.truncated, lost: r.payload_bytes - ((r.payload as any).output as string).length }),
      (info) => {
        expect(info.truncated).toBe(true);
        expect(info.lost).toBe(41741); // 43741 - 2000
      },
    );
  });

  it("truncated=false when payload fits within threshold", () => {
    const record = makeRecord("1", {
      record_type: "tool_result",
      payload: { output: "small" },
      truncated: false,
      payload_bytes: 5,
    });

    scenario(
      () => record,
      (r) => ({ truncated: r.truncated, payload_bytes: r.payload_bytes }),
      (info) => {
        expect(info.truncated).toBe(false);
        expect(info.payload_bytes).toBe(5);
      },
    );
  });

  it("only tool_result records have truncated=true", () => {
    const records = [
      makeRecord("1", { record_type: "tool_call", truncated: false }),
      makeRecord("2", { record_type: "user_message", truncated: false }),
      makeRecord("3", { record_type: "assistant_message", truncated: false }),
      makeRecord("4", { record_type: "tool_result", truncated: true, payload_bytes: 5000 }),
    ];

    scenario(
      () => records,
      (r) => r.filter((rec) => rec.truncated),
      (truncated) => {
        expect(truncated).toHaveLength(1);
        expect(truncated[0]!.record_type).toBe("tool_result");
      },
    );
  });
});

// ── End-to-end: data flow through pipeline ──────────────────────

describe("End-to-end: truncation data flow", () => {
  it("large tool result: server truncates payload, timeline truncates summary, but payload on record is available", () => {
    // Simulating a server-truncated tool result
    const serverTruncatedOutput = "x".repeat(2000); // server capped at 2000
    const record = makeRecord("1", {
      record_type: "tool_result",
      payload: { output: serverTruncatedOutput, is_error: false },
      truncated: true,
      payload_bytes: 20000, // original was 20KB
    });

    scenario(
      () => [record],
      (records) => {
        const rows = toTimelineRows(records);
        const row = rows.find((r) => r.category === "result")!;
        return {
          summaryLength: row.summary.length,
          payloadOutputLength: ((row.record.payload as any).output as string).length,
          serverTruncated: (row.record as WireRecord).truncated,
          originalBytes: (row.record as WireRecord).payload_bytes,
        };
      },
      (info) => {
        // Timeline summary: truncated to <=500
        expect(info.summaryLength).toBeLessThanOrEqual(500);
        // Payload output: server-capped at 2000
        expect(info.payloadOutputLength).toBe(2000);
        // Truncation metadata available for "load full content" feature
        expect(info.serverTruncated).toBe(true);
        expect(info.originalBytes).toBe(20000);
      },
    );
  });
});
