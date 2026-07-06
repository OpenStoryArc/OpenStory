import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { HarnessMessageBlock } from "@/components/events/HarnessMessageBlock";

describe("HarnessMessageBlock", () => {
  it("renders a slash command with its full (untruncated) args", () => {
    const longArgs = "please act as a ux designer ".repeat(20).trim();
    render(<HarnessMessageBlock text={`<command-name>loop</command-name><command-args>${longArgs}</command-args>`} />);
    expect(screen.getByText("/loop")).toBeInTheDocument();
    // full args preserved, not cut off
    expect(screen.getByText(new RegExp(longArgs.slice(0, 40)))).toBeInTheDocument();
    expect(document.querySelector('[data-harness="slash_command"]')).toBeInTheDocument();
  });

  it("renders a task notification with status and summary", () => {
    render(<HarnessMessageBlock text="<task-notification><status>completed</status><summary>Agent finished mapping the UI</summary></task-notification>" />);
    expect(screen.getByText(/background task — completed/i)).toBeInTheDocument();
    expect(screen.getByText(/mapping the UI/i)).toBeInTheDocument();
  });
});
