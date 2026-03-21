import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { EventRow } from "@/components/events/EventRow";
import type { ViewRecord, ToolCall } from "@/types/view-record";

function makeRecord(overrides: Partial<ViewRecord> = {}): ViewRecord {
  return {
    id: "evt-1",
    seq: 1,
    session_id: "s1",
    timestamp: "2025-01-01T12:00:00Z",
    record_type: "tool_call",
    payload: {
      call_id: "c1",
      name: "Read",
      input: { file_path: "/home/user/main.py" },
      raw_input: { file_path: "/home/user/main.py" },
      typed_input: { tool: "read", file_path: "/home/user/main.py" },
    } satisfies ToolCall,
    agent_id: null,
    is_sidechain: false,
    ...overrides,
  };
}

describe("EventRow", () => {
  it("renders tool name and type label", () => {
    const onClick = vi.fn();
    render(
      <EventRow record={makeRecord()} selected={false} onClick={onClick} />,
    );
    expect(screen.getByText("Tool Use")).toBeTruthy();
    expect(screen.getByText("Read")).toBeTruthy();
  });

  it("calls onClick with record id when clicked", () => {
    const onClick = vi.fn();
    render(
      <EventRow record={makeRecord()} selected={false} onClick={onClick} />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onClick).toHaveBeenCalledWith("evt-1");
  });

  it("shows selected state", () => {
    const onClick = vi.fn();
    const { container } = render(
      <EventRow record={makeRecord()} selected={true} onClick={onClick} />,
    );
    const button = container.querySelector("button");
    expect(button?.className).toContain("bg-[#2f3348]");
  });

  it("adds colored left border for git commands", () => {
    const onClick = vi.fn();
    const gitRecord = makeRecord({
      payload: {
        call_id: "c2",
        name: "Bash",
        input: { command: "git commit -m 'fix'" },
        raw_input: { command: "git commit -m 'fix'" },
        typed_input: { tool: "bash", command: "git commit -m 'fix'" },
      } satisfies ToolCall,
    });
    const { container } = render(
      <EventRow record={gitRecord} selected={false} onClick={onClick} />,
    );
    const button = container.querySelector("button");
    // jsdom normalizes hex to rgb
    expect(button?.style.borderLeftColor).toBe("rgb(158, 206, 106)"); // commit green #9ece6a
    expect(button?.style.borderLeftWidth).toBe("3px");
  });

  it("adds red background tint for destructive git commands", () => {
    const onClick = vi.fn();
    const gitRecord = makeRecord({
      payload: {
        call_id: "c3",
        name: "Bash",
        input: { command: "git push --force origin main" },
        raw_input: { command: "git push --force origin main" },
        typed_input: { tool: "bash", command: "git push --force origin main" },
      } satisfies ToolCall,
    });
    const { container } = render(
      <EventRow record={gitRecord} selected={false} onClick={onClick} />,
    );
    const button = container.querySelector("button");
    expect(button?.className).toContain("bg-[#f7768e]/10");
  });

  it("does not add git styling for non-git bash commands", () => {
    const onClick = vi.fn();
    const bashRecord = makeRecord({
      payload: {
        call_id: "c4",
        name: "Bash",
        input: { command: "npm test" },
        raw_input: { command: "npm test" },
        typed_input: { tool: "bash", command: "npm test" },
      } satisfies ToolCall,
    });
    const { container } = render(
      <EventRow record={bashRecord} selected={false} onClick={onClick} />,
    );
    const button = container.querySelector("button");
    expect(button?.style.borderLeftColor).toBe("");
  });
});
