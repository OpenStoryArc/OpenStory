import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SubagentsSection } from "@/components/viz/SubagentsSection";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, seq: number, payload: unknown): WireRecord {
  return {
    id: `e${seq}`, seq, session_id: "s1", timestamp: `2026-06-30T10:00:0${seq}.000Z`,
    record_type, payload: payload as WireRecord["payload"], origin_agent: "claude-code",
    agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50,
  };
}

const RECORDS: WireRecord[] = [
  rec("tool_call", 1, { call_id: "c1", name: "Agent", typed_input: { tool: "agent", description: "Map the UI codebase", subagent_type: "Explore", prompt: "x" } }),
  rec("tool_result", 2, { call_id: "c1", is_error: false, output: JSON.stringify([{ type: "text", text: "agentId: ada5a6f42bd70f812" }]) }),
];

describe("SubagentsSection", () => {
  it("lists spawned subagents with type + description", () => {
    render(<SubagentsSection records={RECORDS} />);
    expect(screen.getByTestId("subagents-section")).toHaveTextContent(/Subagents · 1/);
    expect(screen.getByText("Map the UI codebase")).toBeInTheDocument();
    expect(screen.getByText("Explore")).toBeInTheDocument();
  });

  it("links each subagent to its child session and fires onOpen", () => {
    const onOpen = vi.fn();
    render(<SubagentsSection records={RECORDS} onOpen={onOpen} />);
    fireEvent.click(screen.getByText("Map the UI codebase"));
    expect(onOpen).toHaveBeenCalledWith("agent-ada5a6f42bd70f812");
  });

  it("renders nothing when the session spawned no subagents", () => {
    const { container } = render(<SubagentsSection records={[rec("tool_call", 1, { call_id: "b", name: "Bash", typed_input: { tool: "bash", command: "ls" } })]} />);
    expect(container).toBeEmptyDOMElement();
  });
});
