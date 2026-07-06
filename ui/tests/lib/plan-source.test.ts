/** The plan→turn canopy edge: a plan knows the ExitPlanMode call that
 *  authored it. From there the existing #/story/SES/event/ID deep-link
 *  lands on the authoring turn. Title match first, closest-in-time
 *  ExitPlanMode as the fallback (titles are derived text, times are data). */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { planSourceEventId } from "@/lib/plan-source";
import type { WireRecord } from "@/types/wire-record";

function exitPlan(id: string, ts: string, plan: string): WireRecord {
  return {
    id,
    seq: 1,
    session_id: "s",
    timestamp: ts,
    record_type: "tool_call",
    payload: { name: "ExitPlanMode", raw_input: { plan }, input: {}, call_id: id },
  } as unknown as WireRecord;
}

function other(id: string, ts: string): WireRecord {
  return {
    id,
    seq: 2,
    session_id: "s",
    timestamp: ts,
    record_type: "assistant_message",
    payload: {},
  } as unknown as WireRecord;
}

describe("when a plan's title matches an ExitPlanMode call", () => {
  it("should name that call's event", () =>
    scenario(
      () => ({
        records: [
          other("e0", "2026-07-04T10:00:00Z"),
          exitPlan("e1", "2026-07-04T10:01:00Z", "# Fix the auth bug\ndetails"),
          exitPlan("e2", "2026-07-04T11:00:00Z", "# Ship the release\ndetails"),
        ],
        plan: { title: "Fix the auth bug", timestamp: "2026-07-04T10:01:00Z" },
      }),
      ({ records, plan }) => planSourceEventId(records, plan),
      (id) => expect(id).toBe("e1"),
    ));
});

describe("when titles drift, but the timestamp is near an ExitPlanMode call", () => {
  it("should fall back to the closest-in-time authoring call", () =>
    scenario(
      () => ({
        records: [
          exitPlan("e1", "2026-07-04T10:01:00Z", "# One phrasing"),
          exitPlan("e2", "2026-07-04T11:00:00Z", "# Another phrasing"),
        ],
        plan: { title: "A title the extractor never produced", timestamp: "2026-07-04T10:01:03Z" },
      }),
      ({ records, plan }) => planSourceEventId(records, plan),
      (id) => expect(id).toBe("e1"),
    ));
});

describe("when the session has no ExitPlanMode calls", () => {
  it("should offer no source (null, not a wrong guess)", () =>
    scenario(
      () => ({
        records: [other("e0", "2026-07-04T10:00:00Z")],
        plan: { title: "Anything", timestamp: "2026-07-04T10:00:00Z" },
      }),
      ({ records, plan }) => planSourceEventId(records, plan),
      (id) => expect(id).toBeNull(),
    ));
});
