import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { DurationBeeswarm } from "@/components/canvas/DurationBeeswarm";
import { AgentProjectMatrix } from "@/components/canvas/AgentProjectMatrix";
import type { StorySession } from "@/lib/story-api";

/** Canvas modes must DRILL — every datum leads to its sessions. Durations dots
 *  open the session panel; Agents×Projects cells open Explore filtered to
 *  exactly that cell's sessions. */

const SESSIONS: StorySession[] = [
  {
    session_id: "s-long",
    start_time: "2026-06-10T09:00:00.000Z",
    last_event: "2026-06-10T12:00:00.000Z",
    event_count: 500,
    origin_agent: "claude-code",
    project_name: "OpenStory",
    label: "long build",
  },
  {
    session_id: "s-short",
    start_time: "2026-06-11T14:00:00.000Z",
    last_event: "2026-06-11T14:05:00.000Z",
    event_count: 12,
    origin_agent: "codex",
    project_name: "OtherProj",
    label: "quick fix",
  },
];

describe("when a Durations dot is clicked", () => {
  it("should report the session id so the canvas opens its panel", () => {
    const onOpenSession = vi.fn();
    const { container } = render(<DurationBeeswarm sessions={SESSIONS} onOpenSession={onOpenSession} />);
    const dot = container.querySelector("circle[data-session-id='s-long']") as SVGElement;
    expect(dot).toBeTruthy();
    fireEvent.click(dot);
    expect(onOpenSession).toHaveBeenCalledWith("s-long");
  });
});

describe("when an Agents×Projects cell is clicked", () => {
  it("should report the cell's agent + project for a filtered Explore drill", () => {
    const onOpenCell = vi.fn();
    const { container } = render(<AgentProjectMatrix sessions={SESSIONS} onOpenCell={onOpenCell} />);
    const cell = container.querySelector("[data-cell='claude-code|OpenStory']") as SVGElement;
    expect(cell).toBeTruthy();
    fireEvent.click(cell);
    expect(onOpenCell).toHaveBeenCalledWith("claude-code", "OpenStory");
  });

  it("should not report empty cells (nothing behind them to drill into)", () => {
    const onOpenCell = vi.fn();
    const { container } = render(<AgentProjectMatrix sessions={SESSIONS} onOpenCell={onOpenCell} />);
    const empty = container.querySelector("[data-cell='claude-code|OtherProj']") as SVGElement;
    expect(empty).toBeTruthy();
    fireEvent.click(empty);
    expect(onOpenCell).not.toHaveBeenCalled();
  });
});
