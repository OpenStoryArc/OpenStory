/** The toolcall↔result canopy edge: a call and its result are one round
 *  trip joined by call_id. This pure map gives each record the EVENT id of
 *  its partner, so a card can jump straight across. */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { toolPairMap } from "@/lib/tool-pair";
import type { WireRecord } from "@/types/wire-record";

function rec(id: string, type: string, callId?: string): WireRecord {
  return {
    id,
    seq: 1,
    session_id: "s",
    timestamp: "2026-07-04T10:00:00Z",
    record_type: type,
    payload: callId ? { call_id: callId } : {},
  } as unknown as WireRecord;
}

describe("when a session has tool calls and results", () => {
  it("should pair each call with its result in both directions", () =>
    scenario(
      () => [
        rec("e1", "tool_call", "c1"),
        rec("e2", "tool_result", "c1"),
        rec("e3", "tool_call", "c2"),
        rec("e4", "tool_result", "c2"),
      ],
      (records) => toolPairMap(records),
      (pairs) => {
        expect(pairs.get("e1")).toBe("e2");
        expect(pairs.get("e2")).toBe("e1");
        expect(pairs.get("e3")).toBe("e4");
        expect(pairs.get("e4")).toBe("e3");
      },
    ));

  it("should leave an unanswered call unpaired (no phantom partner)", () =>
    scenario(
      () => [rec("e1", "tool_call", "c1"), rec("e2", "assistant_message")],
      (records) => toolPairMap(records),
      (pairs) => {
        expect(pairs.has("e1")).toBe(false);
        expect(pairs.size).toBe(0);
      },
    ));

  it("should ignore records without a call_id", () =>
    scenario(
      () => [rec("e1", "tool_call"), rec("e2", "tool_result")],
      (records) => toolPairMap(records),
      (pairs) => expect(pairs.size).toBe(0),
    ));
});
