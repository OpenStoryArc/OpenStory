import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { SynopsisCard } from "@/components/session/SynopsisCard";
import type { SessionSynopsis } from "@/lib/session-detail";

function synopsis(over: Partial<SessionSynopsis> = {}): SessionSynopsis {
  return {
    session_id: "s1", label: null, project_id: null, project_name: "OpenStory",
    event_count: 100, tool_count: 20, error_count: 0,
    first_event: "2026-06-30T10:00:00Z", last_event: "2026-06-30T11:00:00Z",
    duration_secs: 3600, top_tools: [{ tool: "Bash", count: 5 }], ...over,
  };
}

describe("SynopsisCard", () => {
  it("humanizes a harness-wrapper label instead of showing raw XML", () => {
    render(<SynopsisCard synopsis={synopsis({ label: "<command-message>loop</command-message>\n<command-n" })} />);
    // the exact leak Max saw on the Explore page
    expect(screen.getByText("/loop")).toBeInTheDocument();
    expect(screen.queryByText(/command-message/)).toBeNull();
  });

  it("shows an ordinary label unchanged", () => {
    render(<SynopsisCard synopsis={synopsis({ label: "fix the login bug" })} />);
    expect(screen.getByText("fix the login bug")).toBeInTheDocument();
  });
});
