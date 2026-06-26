//! Spec: WatchBanner — the "Agent is watching" follow prompt shown when a
//! focus message arrives while the user is NOT on the Live tab. One click to
//! follow, one to dismiss; never navigates on its own.

import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { WatchBanner } from "@/components/WatchBanner";
import type { FocusMessage } from "@/types/websocket";

const msg: FocusMessage = {
  kind: "focus",
  session_id: "sess-a1",
  label: "Where are we at?",
  project_name: "agent-harness",
  host: "a1",
  user: "max",
};

describe("WatchBanner", () => {
  it("identifies the session the agent is watching", () => {
    render(<WatchBanner message={msg} onFollow={() => {}} onDismiss={() => {}} />);
    const el = screen.getByTestId("watch-banner");
    expect(el.textContent).toContain("agent-harness");
    expect(el.textContent).toContain("a1");
    expect(el.textContent).toContain("max");
  });

  it("calls onFollow when Follow is clicked", () => {
    const onFollow = vi.fn();
    render(<WatchBanner message={msg} onFollow={onFollow} onDismiss={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: /follow/i }));
    expect(onFollow).toHaveBeenCalledTimes(1);
  });

  it("calls onDismiss when Dismiss is clicked", () => {
    const onDismiss = vi.fn();
    render(<WatchBanner message={msg} onFollow={() => {}} onDismiss={onDismiss} />);
    fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("falls back to the session id when no label/project is present", () => {
    const bare: FocusMessage = { kind: "focus", session_id: "sess-xyz-789" };
    render(<WatchBanner message={bare} onFollow={() => {}} onDismiss={() => {}} />);
    expect(screen.getByTestId("watch-banner").textContent).toContain("sess-xyz");
  });
});
