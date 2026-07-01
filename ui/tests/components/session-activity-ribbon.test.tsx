import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { SessionActivityRibbon } from "@/components/viz/SessionActivityRibbon";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, timestamp: string, seq: number, payload: unknown = {}): WireRecord {
  return {
    id: `e${seq}`,
    seq,
    session_id: "s1",
    timestamp,
    record_type,
    payload: payload as WireRecord["payload"],
    origin_agent: "claude-code",
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 100,
  };
}

const RECORDS: WireRecord[] = [
  rec("user_message", "2026-06-30T10:00:00.000Z", 1),
  rec("tool_call", "2026-06-30T10:00:05.000Z", 2, { name: "Read" }),
  rec("tool_result", "2026-06-30T10:00:06.000Z", 3, { is_error: true, call_id: "c1" }),
  rec("assistant_message", "2026-06-30T10:00:10.000Z", 4),
];

describe("SessionActivityRibbon", () => {
  it("renders one mark per drawable event", () => {
    render(<SessionActivityRibbon records={RECORDS} width={600} />);
    const marks = document.querySelectorAll('[data-ribbon-mark]');
    expect(marks).toHaveLength(4);
  });

  it("labels the lanes that are present", () => {
    render(<SessionActivityRibbon records={RECORDS} width={600} />);
    // user, tool, assistant lanes present (no reasoning/system marks here)
    expect(screen.getByText("user")).toBeInTheDocument();
    expect(screen.getByText("tool")).toBeInTheDocument();
    expect(screen.getByText("assistant")).toBeInTheDocument();
  });

  it("surfaces the error count in the summary", () => {
    render(<SessionActivityRibbon records={RECORDS} width={600} />);
    expect(screen.getByText(/1 error/i)).toBeInTheDocument();
  });

  it("renders an empty-state message when there are no events", () => {
    render(<SessionActivityRibbon records={[]} width={600} />);
    expect(screen.getByText(/no activity/i)).toBeInTheDocument();
  });
});
