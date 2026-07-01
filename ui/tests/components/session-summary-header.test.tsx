import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { SessionSummaryHeader } from "@/components/viz/SessionSummaryHeader";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, ts: string, seq: number, payload: unknown = {}): WireRecord {
  return {
    id: `e${seq}`, seq, session_id: "s1", timestamp: ts, record_type,
    payload: payload as WireRecord["payload"], origin_agent: "claude-code",
    agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null,
    truncated: false, payload_bytes: 50,
  };
}
const T = (s: number) => `2026-06-30T10:00:${String(s).padStart(2, "0")}.000Z`;

const RECORDS: WireRecord[] = [
  rec("assistant_message", T(0), 1, { model: "claude-opus-4-8", content: [] }),
  rec("tool_call", T(1), 2, { call_id: "c1", name: "Bash", typed_input: { tool: "bash", command: "x" } }),
  rec("tool_result", T(2), 3, { call_id: "c1", is_error: true }),
  rec("turn_end", T(30), 4, {}),
];

describe("SessionSummaryHeader", () => {
  it("shows tool count, duration, and the model", () => {
    const { container } = render(<SessionSummaryHeader records={RECORDS} />);
    expect(container).toHaveTextContent(/1 tool/i);
    expect(container).toHaveTextContent(/30s/);
    expect(container).toHaveTextContent(/opus-4-8/);
  });

  it("surfaces errors as a distinct, first-class stat", () => {
    render(<SessionSummaryHeader records={RECORDS} />);
    const err = screen.getByTestId("summary-errors");
    expect(err).toHaveTextContent(/1 error/i);
  });

  it("hides the error stat when there are none", () => {
    render(<SessionSummaryHeader records={[rec("user_message", T(0), 1)]} />);
    expect(screen.queryByTestId("summary-errors")).toBeNull();
  });
});
