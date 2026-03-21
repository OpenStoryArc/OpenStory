import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SessionCard } from "@/components/session/SessionCard";
import type { SessionSummary } from "@/types/session";

function makeSession(overrides: Partial<SessionSummary> = {}): SessionSummary {
  return {
    session_id: "s1",
    status: "ongoing",
    start_time: "2026-01-01T00:00:00Z",
    event_count: 0,
    ...overrides,
  };
}

describe("SessionCard", () => {
  it("renders session status", () => {
    render(
      <SessionCard
        session={makeSession({ status: "ongoing" })}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("ongoing")).toBeTruthy();
  });

  it("renders first_prompt when available", () => {
    render(
      <SessionCard
        session={makeSession({ first_prompt: "Hello world" })}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("Hello world")).toBeTruthy();
  });

  it("renders 'No prompt yet' when first_prompt is absent", () => {
    render(
      <SessionCard
        session={makeSession()}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("No prompt yet")).toBeTruthy();
  });

  it("renders event count", () => {
    render(
      <SessionCard
        session={makeSession({ event_count: 15 })}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("15 events")).toBeTruthy();
  });

  it("renders model when available", () => {
    render(
      <SessionCard
        session={makeSession({ model: "claude-sonnet-4-6" })}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("claude-sonnet-4-6")).toBeTruthy();
  });

  it("renders duration when available", () => {
    render(
      <SessionCard
        session={makeSession({ duration_ms: 65000 })}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("1m 5s")).toBeTruthy();
  });

  it("applies selected background style", () => {
    const { container } = render(
      <SessionCard
        session={makeSession()}
        selected={true}
        onSelect={vi.fn()}
      />,
    );
    const button = container.querySelector("button");
    expect(button?.className).toContain("bg-[#2f3348]");
  });

  it("calls onSelect with session_id when clicked", () => {
    const onSelect = vi.fn();
    render(
      <SessionCard
        session={makeSession({ session_id: "abc-123" })}
        selected={false}
        onSelect={onSelect}
      />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onSelect).toHaveBeenCalledWith("abc-123");
  });

  it("truncates long first_prompt", () => {
    const longPrompt = "A".repeat(100);
    render(
      <SessionCard
        session={makeSession({ first_prompt: longPrompt })}
        selected={false}
        onSelect={vi.fn()}
      />,
    );
    // truncate(longPrompt, 80) => 79 chars + ellipsis
    const truncated = screen.getByText(/^A+\u2026$/);
    expect(truncated).toBeTruthy();
  });
});
