import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { CommandPalette, buildPaletteItems } from "@/components/command/CommandPalette";
import type { StorySession } from "@/lib/story-api";

const SESSIONS: StorySession[] = [
  { session_id: "abc-123", label: "fix the login bug", project_name: "OpenStory", branch: "master", user: "max" },
  { session_id: "def-456", label: "<command-message>loop</command-message>", project_name: "OpenStory", branch: "feat/ui", user: "katie" },
  { session_id: "agent-999", label: "subagent noise", project_name: "OpenStory" },
];

describe("buildPaletteItems", () => {
  it("includes tab items plus one item per non-subagent session, with cleaned titles", () => {
    const items = buildPaletteItems(SESSIONS);
    expect(items.find((i) => i.id === "tab-explore")).toBeTruthy();
    // The Overview tab merged into Explore — no orphaned palette entry.
    expect(items.find((i) => i.id === "tab-overview")).toBeUndefined();
    expect(items.filter((i) => i.group === "Sessions")).toHaveLength(2); // agent-999 excluded
    // harness-wrapper label humanized
    expect(items.find((i) => i.id === "session-def-456")?.title).toBe("/loop");
  });
});

describe("CommandPalette", () => {
  it("opens on Cmd-K, filters as you type, and navigates on Enter", () => {
    const onNavigate = vi.fn();
    render(<CommandPalette sessions={SESSIONS} onNavigate={onNavigate} />);

    // closed initially
    expect(screen.queryByTestId("command-palette")).toBeNull();

    // open with Cmd-K
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(screen.getByTestId("command-palette")).toBeInTheDocument();

    const input = screen.getByPlaceholderText(/jump to a session or view/i);
    fireEvent.change(input, { target: { value: "login" } });

    // only the matching session survives
    expect(document.querySelector('[data-palette-item="session-abc-123"]')).toBeInTheDocument();
    expect(document.querySelector('[data-palette-item="session-def-456"]')).toBeNull();

    // Enter opens the top result in Explore
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onNavigate).toHaveBeenCalledWith({ view: "explore", sessionId: "abc-123" });
    // palette closes after navigation
    expect(screen.queryByTestId("command-palette")).toBeNull();
  });

  it("surfaces recently-viewed sessions first on an empty query", () => {
    render(<CommandPalette sessions={SESSIONS} onNavigate={() => {}} recentIds={["abc-123"]} />);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    // recent session appears before typing anything
    const recent = document.querySelector('[data-palette-item="session-abc-123"]');
    expect(recent).toBeInTheDocument();
    expect(recent).toHaveTextContent(/recent/i);
    // pressing Enter opens the top (recent) result
    // (first item is the recent session, not a tab)
    const items = document.querySelectorAll("[data-palette-item]");
    expect(items[0]?.getAttribute("data-palette-item")).toBe("session-abc-123");
  });

  it("navigates to a tab when a Navigate item is chosen", () => {
    const onNavigate = vi.fn();
    render(<CommandPalette sessions={SESSIONS} onNavigate={onNavigate} />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const input = screen.getByPlaceholderText(/jump to a session or view/i);
    // "overview" still finds the sessions browser — it fuzzy-matches Explore.
    fireEvent.change(input, { target: { value: "overview" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onNavigate).toHaveBeenCalledWith({ view: "explore" });
  });
});
