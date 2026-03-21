import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { GitCommandDetail } from "@/components/events/GitCommandDetail";
import type { ViewRecord, ToolCall } from "@/types/view-record";

function makeGitRecord(command: string, overrides: Partial<ViewRecord> = {}): ViewRecord {
  return {
    id: "evt-git-1",
    seq: 1,
    session_id: "s1",
    timestamp: "2026-01-15T14:30:00Z",
    record_type: "tool_call",
    agent_id: null,
    is_sidechain: false,
    payload: {
      call_id: "c1",
      name: "Bash",
      input: { command },
      raw_input: { command },
      typed_input: { tool: "bash", command },
    } satisfies ToolCall,
    ...overrides,
  };
}

describe("GitCommandDetail", () => {
  it("renders the risk badge with correct label", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git push --force origin main")} onClose={vi.fn()} />,
    );
    expect(screen.getByText("Destructive")).toBeTruthy();
  });

  it("renders the full command in monospace", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git commit -m 'fix bug'")} onClose={vi.fn()} />,
    );
    expect(screen.getByText("git commit -m 'fix bug'")).toBeTruthy();
  });

  it("renders warning callout for force push", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git push --force origin main")} onClose={vi.fn()} />,
    );
    expect(screen.getByText(/Force push overwrites remote history/)).toBeTruthy();
  });

  it("renders warning callout for reset --hard", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git reset --hard HEAD~1")} onClose={vi.fn()} />,
    );
    expect(screen.getByText(/Discards all uncommitted changes/)).toBeTruthy();
  });

  it("renders warning callout for clean -f", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git clean -fd")} onClose={vi.fn()} />,
    );
    expect(screen.getByText(/Permanently deletes untracked files/)).toBeTruthy();
  });

  it("renders warning callout for branch -D", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git branch -D old-branch")} onClose={vi.fn()} />,
    );
    expect(screen.getByText(/Deletes branch even if not fully merged/)).toBeTruthy();
  });

  it("renders warning callout for checkout -- .", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git checkout -- .")} onClose={vi.fn()} />,
    );
    expect(screen.getByText(/Discards all unstaged changes/)).toBeTruthy();
  });

  it("renders warning callout for commit --amend", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git commit --amend")} onClose={vi.fn()} />,
    );
    expect(screen.getByText(/Rewrites the previous commit/)).toBeTruthy();
  });

  it("renders warning callout for rebase", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git rebase main")} onClose={vi.fn()} />,
    );
    expect(screen.getByText(/Rewrites commit history/)).toBeTruthy();
  });

  it("does not render warning callout for safe commands", () => {
    const { container } = render(
      <GitCommandDetail record={makeGitRecord("git status")} onClose={vi.fn()} />,
    );
    expect(container.querySelector("[data-testid='git-warning']")).toBeNull();
  });

  it("renders commit risk badge for commit commands", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git commit -m 'fix'")} onClose={vi.fn()} />,
    );
    expect(screen.getByText("Commit")).toBeTruthy();
  });

  it("renders safe risk badge for safe commands", () => {
    render(
      <GitCommandDetail record={makeGitRecord("git status")} onClose={vi.fn()} />,
    );
    expect(screen.getByText("Safe")).toBeTruthy();
  });
});
