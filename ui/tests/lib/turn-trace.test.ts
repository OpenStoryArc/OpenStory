import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildTurnTrace } from "@/lib/turn-trace";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, ts: string, seq: number, payload: unknown, extra: Partial<WireRecord> = {}): WireRecord {
  return {
    id: `e${seq}`,
    seq,
    session_id: "s1",
    timestamp: ts,
    record_type,
    payload: payload as WireRecord["payload"],
    origin_agent: "claude-code",
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 50,
    ...extra,
  };
}

const T = (s: number) => `2026-06-30T10:00:${String(s).padStart(2, "0")}.000Z`;

describe("buildTurnTrace", () => {
  it("pairs a tool_call with its tool_result by call_id and computes duration", () => {
    scenario(
      () => [
        rec("tool_call", T(0), 1, { call_id: "c1", name: "Bash", typed_input: { tool: "bash", command: "ls" } }),
        rec("tool_result", T(3), 2, { call_id: "c1", is_error: false }),
      ],
      (records) => buildTurnTrace(records),
      (model) => {
        expect(model.spans).toHaveLength(1);
        const span = model.spans[0]!;
        expect(span.name).toBe("Bash");
        expect(span.durationMs).toBe(3000);
        expect(span.isError).toBe(false);
        expect(span.detail).toBe("ls");
      },
    );
  });

  it("pairs correctly even when records are out of seq/time order", () => {
    scenario(
      () => [
        rec("tool_result", T(5), 2, { call_id: "c1", is_error: false }),
        rec("tool_call", T(2), 1, { call_id: "c1", name: "Read", typed_input: { tool: "read", file_path: "/a" } }),
      ],
      (records) => buildTurnTrace(records),
      (model) => expect(model.spans[0]!.durationMs).toBe(3000),
    );
  });

  it("marks a call with no result as unresolved (null duration)", () => {
    scenario(
      () => [rec("tool_call", T(0), 1, { call_id: "orphan", name: "Grep", typed_input: { tool: "grep", pattern: "x" } })],
      (records) => buildTurnTrace(records),
      (model) => {
        expect(model.spans[0]!.endMs).toBeNull();
        expect(model.spans[0]!.durationMs).toBeNull();
      },
    );
  });

  it("ignores an orphan tool_result with no matching call", () => {
    scenario(
      () => [rec("tool_result", T(1), 1, { call_id: "ghost", is_error: false })],
      (records) => buildTurnTrace(records),
      (model) => expect(model.spans).toHaveLength(0),
    );
  });

  it("flags errored tool results and counts the error", () => {
    scenario(
      () => [
        rec("tool_call", T(0), 1, { call_id: "c1", name: "Bash", typed_input: { tool: "bash", command: "boom" } }),
        rec("tool_result", T(1), 2, { call_id: "c1", is_error: true }),
      ],
      (records) => buildTurnTrace(records),
      (model) => {
        expect(model.spans[0]!.isError).toBe(true);
        expect(model.errorCount).toBe(1);
      },
    );
  });

  it("orders spans by start time and identifies the slowest", () => {
    scenario(
      () => [
        rec("tool_call", T(0), 1, { call_id: "fast", name: "Read", typed_input: { tool: "read", file_path: "/a" } }),
        rec("tool_result", T(1), 2, { call_id: "fast", is_error: false }),
        rec("tool_call", T(2), 3, { call_id: "slow", name: "Bash", typed_input: { tool: "bash", command: "build" } }),
        rec("tool_result", T(9), 4, { call_id: "slow", is_error: false }),
      ],
      (records) => buildTurnTrace(records),
      (model) => {
        expect(model.spans.map((s) => s.callId)).toEqual(["fast", "slow"]);
        expect(model.slowest?.callId).toBe("slow");
        expect(model.slowest?.durationMs).toBe(7000);
      },
    );
  });

  it("computes the trace time domain across all spans", () => {
    scenario(
      () => [
        rec("tool_call", T(2), 1, { call_id: "c1", name: "Read", typed_input: { tool: "read", file_path: "/a" } }),
        rec("tool_result", T(4), 2, { call_id: "c1", is_error: false }),
        rec("tool_call", T(6), 3, { call_id: "c2", name: "Edit", typed_input: { tool: "edit", file_path: "/b" } }),
        rec("tool_result", T(8), 4, { call_id: "c2", is_error: false }),
      ],
      (records) => buildTurnTrace(records),
      (model) => {
        expect(model.domain[0]).toBe(Date.parse(T(2)));
        expect(model.domain[1]).toBe(Date.parse(T(8)));
        expect(model.totalMs).toBe(6000);
      },
    );
  });
});
