import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { firstErrorEventId } from "@/lib/session-summary";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, seq: number, payload: unknown = {}): WireRecord {
  return {
    id: `e${seq}`, seq, session_id: "s1", timestamp: `2026-06-30T10:00:${String(seq).padStart(2, "0")}.000Z`,
    record_type, payload: payload as WireRecord["payload"], origin_agent: "claude-code",
    agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50,
  };
}

describe("firstErrorEventId", () => {
  it("returns the id of the earliest error record", () => {
    scenario(
      () => [
        rec("user_message", 1),
        rec("error", 2, { code: "x", message: "boom" }),
        rec("error", 3, { code: "y", message: "later" }),
      ],
      (records) => firstErrorEventId(records),
      (id) => expect(id).toBe("e2"),
    );
  });

  it("counts an errored tool_result as a failure and picks the earliest by seq", () => {
    scenario(
      () => [
        rec("tool_result", 5, { call_id: "c2", is_error: true }),
        rec("tool_result", 2, { call_id: "c1", is_error: true }),
        rec("tool_result", 8, { call_id: "c3", is_error: false }),
      ],
      (records) => firstErrorEventId(records),
      (id) => expect(id).toBe("e2"),
    );
  });

  it("returns null when there are no failures", () => {
    scenario(
      () => [rec("user_message", 1), rec("tool_result", 2, { call_id: "c1", is_error: false })],
      (records) => firstErrorEventId(records),
      (id) => expect(id).toBeNull(),
    );
  });
});
