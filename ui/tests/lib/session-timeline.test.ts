import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildTimelineModel, laneFor, LANE_ORDER } from "@/lib/session-timeline";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";
import { synthBatch } from "../fixtures/synth";

/** Minimal WireRecord factory for exact, hand-built fixtures. */
function rec(
  partial: Partial<WireRecord> & { record_type: RecordType; timestamp: string },
): WireRecord {
  return {
    id: partial.id ?? Math.random().toString(36).slice(2),
    seq: partial.seq ?? 0,
    session_id: "s1",
    origin_agent: "claude-code",
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: partial.payload_bytes ?? 100,
    payload: partial.payload ?? ({} as WireRecord["payload"]),
    ...partial,
  };
}

const T0 = "2026-06-30T10:00:00.000Z";
const T1 = "2026-06-30T10:00:10.000Z";
const T2 = "2026-06-30T10:00:20.000Z";

describe("laneFor", () => {
  it("maps each record type to its swimlane (token_usage has no mark)", () => {
    scenario(
      () => undefined,
      () => ({
        user: laneFor("user_message"),
        reasoning: laneFor("reasoning"),
        assistant: laneFor("assistant_message"),
        toolCall: laneFor("tool_call"),
        toolResult: laneFor("tool_result"),
        error: laneFor("error"),
        system: laneFor("turn_end"),
        token: laneFor("token_usage"),
      }),
      (r) => {
        expect(r.user).toBe("user");
        expect(r.reasoning).toBe("reasoning");
        expect(r.assistant).toBe("assistant");
        expect(r.toolCall).toBe("tool");
        expect(r.toolResult).toBe("tool");
        expect(r.error).toBe("system");
        expect(r.system).toBe("system");
        expect(r.token).toBeNull();
      },
    );
  });
});

describe("buildTimelineModel", () => {
  describe("when the session is empty", () => {
    it("returns a zero-width domain and no events", () => {
      scenario(
        () => [] as WireRecord[],
        (records) => buildTimelineModel(records),
        (model) => {
          expect(model.events).toHaveLength(0);
          expect(model.durationMs).toBe(0);
          expect(model.totalTokens).toBe(0);
          expect(model.lanes).toHaveLength(0);
        },
      );
    });
  });

  describe("when the session spans time", () => {
    it("computes the domain from first to last event timestamp", () => {
      scenario(
        () => [
          rec({ record_type: "user_message", timestamp: T0, seq: 1 }),
          rec({ record_type: "assistant_message", timestamp: T2, seq: 2 }),
        ],
        (records) => buildTimelineModel(records),
        (model) => {
          expect(model.domain[0]).toBe(Date.parse(T0));
          expect(model.domain[1]).toBe(Date.parse(T2));
          expect(model.durationMs).toBe(20_000);
        },
      );
    });
  });

  describe("lane ordering", () => {
    it("lists only the lanes present, in canonical order", () => {
      scenario(
        () => [
          rec({ record_type: "tool_call", timestamp: T1, seq: 2, payload: { name: "Read" } as WireRecord["payload"] }),
          rec({ record_type: "user_message", timestamp: T0, seq: 1 }),
        ],
        (records) => buildTimelineModel(records),
        (model) => {
          // user comes before tool in LANE_ORDER regardless of insertion order
          expect(model.lanes).toEqual(["user", "tool"]);
          expect(LANE_ORDER.indexOf("user")).toBeLessThan(LANE_ORDER.indexOf("tool"));
        },
      );
    });
  });

  describe("token burn", () => {
    it("accumulates output tokens across turn-scoped usage, ignoring session_total snapshots", () => {
      scenario(
        () => [
          rec({ record_type: "token_usage", timestamp: T0, seq: 1, payload: { output_tokens: 100, scope: "turn" } as WireRecord["payload"] }),
          rec({ record_type: "token_usage", timestamp: T1, seq: 2, payload: { output_tokens: 9999, scope: "session_total" } as WireRecord["payload"] }),
          rec({ record_type: "token_usage", timestamp: T2, seq: 3, payload: { output_tokens: 50, scope: "turn" } as WireRecord["payload"] }),
        ],
        (records) => buildTimelineModel(records),
        (model) => {
          expect(model.tokenSeries.map((p) => p.cumulative)).toEqual([100, 150]);
          expect(model.totalTokens).toBe(150);
        },
      );
    });
  });

  describe("errors", () => {
    it("flags error records and errored tool results, and counts them", () => {
      scenario(
        () => [
          rec({ record_type: "tool_result", timestamp: T0, seq: 1, payload: { is_error: true, call_id: "c1" } as WireRecord["payload"] }),
          rec({ record_type: "error", timestamp: T1, seq: 2, payload: { code: "x", message: "boom" } as WireRecord["payload"] }),
          rec({ record_type: "tool_result", timestamp: T2, seq: 3, payload: { is_error: false, call_id: "c2" } as WireRecord["payload"] }),
        ],
        (records) => buildTimelineModel(records),
        (model) => {
          expect(model.errorCount).toBe(2);
          const errored = model.events.filter((e) => e.isError).map((e) => e.seq);
          expect(errored).toEqual([1, 2]);
        },
      );
    });
  });

  describe("event ordering and lane counts", () => {
    it("sorts events by time and counts marks per lane", () => {
      scenario(
        () => [
          rec({ record_type: "assistant_message", timestamp: T2, seq: 3 }),
          rec({ record_type: "user_message", timestamp: T0, seq: 1 }),
          rec({ record_type: "tool_call", timestamp: T1, seq: 2, payload: { name: "Bash" } as WireRecord["payload"] }),
        ],
        (records) => buildTimelineModel(records),
        (model) => {
          expect(model.events.map((e) => e.seq)).toEqual([1, 2, 3]);
          expect(model.laneCounts.user).toBe(1);
          expect(model.laneCounts.tool).toBe(1);
          expect(model.laneCounts.assistant).toBe(1);
        },
      );
    });
  });

  describe("robustness on synthetic sessions", () => {
    it("handles a realistic 200-record session without producing invalid marks", () => {
      scenario(
        () => synthBatch({ count: 200, sessions: 1, seed: 7 }),
        (records) => buildTimelineModel(records),
        (model) => {
          // every event has a known lane and a finite timestamp
          expect(model.events.every((e) => LANE_ORDER.includes(e.lane))).toBe(true);
          expect(model.events.every((e) => Number.isFinite(e.t))).toBe(true);
          // token_usage never becomes a mark
          expect(model.events.every((e) => e.recordType !== "token_usage")).toBe(true);
          expect(model.domain[1]).toBeGreaterThanOrEqual(model.domain[0]);
        },
      );
    });
  });
});
