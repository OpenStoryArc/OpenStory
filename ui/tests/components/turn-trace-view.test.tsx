import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TurnTraceView } from "@/components/viz/TurnTraceView";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, ts: string, seq: number, payload: unknown): WireRecord {
  return {
    id: `e${seq}`, seq, session_id: "s1", timestamp: ts, record_type,
    payload: payload as WireRecord["payload"], origin_agent: "claude-code",
    agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null,
    truncated: false, payload_bytes: 50,
  };
}
const T = (s: number) => `2026-06-30T10:00:${String(s).padStart(2, "0")}.000Z`;

const RECORDS: WireRecord[] = [
  rec("tool_call", T(0), 1, { call_id: "fast", name: "Read", typed_input: { tool: "read", file_path: "/a.ts" } }),
  rec("tool_result", T(1), 2, { call_id: "fast", is_error: false }),
  rec("tool_call", T(2), 3, { call_id: "slow", name: "Bash", typed_input: { tool: "bash", command: "cargo build" } }),
  rec("tool_result", T(9), 4, { call_id: "slow", is_error: true }),
];

describe("TurnTraceView", () => {
  it("renders one row per tool span", () => {
    render(<TurnTraceView records={RECORDS} width={600} />);
    expect(document.querySelectorAll("[data-trace-span]")).toHaveLength(2);
  });

  it("surfaces the slowest span in the summary", () => {
    render(<TurnTraceView records={RECORDS} width={600} />);
    expect(screen.getByText(/slowest/i)).toBeInTheDocument();
    // 7s slow Bash span — appears in both the summary and the span's row
    expect(screen.getAllByText(/7s/).length).toBeGreaterThanOrEqual(1);
  });

  it("calls onSelectSpan with the call id when a row is clicked", () => {
    const onSelect = vi.fn();
    render(<TurnTraceView records={RECORDS} width={600} onSelectSpan={onSelect} />);
    fireEvent.click(document.querySelector('[data-trace-span="slow"]')!);
    expect(onSelect).toHaveBeenCalledWith("slow");
  });

  it("renders an empty state when there are no tool calls", () => {
    render(<TurnTraceView records={[]} width={600} />);
    expect(screen.getByText(/no tool calls/i)).toBeInTheDocument();
  });
});
