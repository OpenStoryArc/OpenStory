import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { EventDetail } from "@/components/events/EventDetail";
import type { ViewRecord, ToolCall } from "@/types/view-record";

function makeRecord(overrides: Partial<ViewRecord> = {}): ViewRecord {
  return {
    id: "evt-abcdef12",
    seq: 5,
    session_id: "s1",
    timestamp: "2026-01-15T14:30:00Z",
    record_type: "tool_call",
    payload: {
      call_id: "c1",
      name: "Read",
      input: {},
      raw_input: {},
    } satisfies ToolCall,
    agent_id: null,
    is_sidechain: false,
    ...overrides,
  };
}

describe("EventDetail", () => {
  it("renders record label from record_type", () => {
    render(<EventDetail record={makeRecord()} onClose={vi.fn()} />);
    expect(screen.getByText("Tool Use")).toBeTruthy();
  });

  it("renders truncated record ID", () => {
    render(
      <EventDetail
        record={makeRecord({ id: "abcdef1234567890" })}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("abcdef12")).toBeTruthy();
  });

  it("renders seq number", () => {
    render(
      <EventDetail
        record={makeRecord({ seq: 42 })}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("42")).toBeTruthy();
  });

  it("renders JSON payload data", () => {
    render(
      <EventDetail
        record={makeRecord({
          payload: {
            call_id: "c2",
            name: "Bash",
            input: {},
            raw_input: {},
          } satisfies ToolCall,
        })}
        onClose={vi.fn()}
      />,
    );
    const pre = screen.getByText(/"name": "Bash"/);
    expect(pre).toBeTruthy();
  });

  it("calls onClose when Close button is clicked", () => {
    const onClose = vi.fn();
    render(<EventDetail record={makeRecord()} onClose={onClose} />);
    fireEvent.click(screen.getByText("Close"));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("renders tool name for tool_call records", () => {
    render(
      <EventDetail
        record={makeRecord()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("Read")).toBeTruthy();
  });

  it("renders GitCommandDetail for git bash records", () => {
    render(
      <EventDetail
        record={makeRecord({
          record_type: "tool_call",
          payload: {
            call_id: "c3",
            name: "Bash",
            input: { command: "git push --force origin main" },
            raw_input: { command: "git push --force origin main" },
            typed_input: { tool: "bash", command: "git push --force origin main" },
          } satisfies ToolCall,
        })}
        onClose={vi.fn()}
      />,
    );
    // GitCommandDetail renders "Git Command" heading and risk badge
    expect(screen.getByText("Git Command")).toBeTruthy();
    expect(screen.getByText("Destructive")).toBeTruthy();
  });

  it("renders generic detail for non-git records", () => {
    render(
      <EventDetail
        record={makeRecord({
          record_type: "tool_call",
          payload: {
            call_id: "c4",
            name: "Read",
            input: { file_path: "/foo" },
            raw_input: { file_path: "/foo" },
            typed_input: { tool: "read", file_path: "/foo" },
          } satisfies ToolCall,
        })}
        onClose={vi.fn()}
      />,
    );
    // Generic detail renders "Record Data" section
    expect(screen.getByText("Record Data")).toBeTruthy();
    // Should NOT render "Git Command" heading
    expect(screen.queryByText("Git Command")).toBeNull();
  });
});
