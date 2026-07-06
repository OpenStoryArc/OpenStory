import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ExploreDashboard } from "@/components/explore/ExploreDashboard";
import { computeStats } from "@/lib/sessions-overview";
import type { StorySession } from "@/lib/story-api";

/** The Explore landing (no session selected) — Overview's dashboard reborn:
 *  stats bar, calendar, recent strip, honest error/empty states. */

const SESSIONS: StorySession[] = [
  {
    session_id: "big-one",
    start_time: "2026-06-10T09:00:00.000Z",
    last_event: "2026-06-10T12:00:00.000Z",
    event_count: 500,
    total_input_tokens: 4000,
    total_output_tokens: 6000,
    project_name: "OpenStory",
    status: "completed",
    label: "build the dashboard",
  },
  {
    session_id: "small-one",
    start_time: "2026-06-11T14:00:00.000Z",
    last_event: "2026-06-11T14:05:00.000Z",
    event_count: 12,
    total_output_tokens: 100,
    project_name: "OpenStory",
    status: "ongoing",
    label: "quick fix",
  },
];

function props(over: Partial<Parameters<typeof ExploreDashboard>[0]> = {}) {
  return {
    universe: SESSIONS,
    stats: computeStats(SESSIONS),
    filtersActive: false,
    selectedDay: null,
    loading: false,
    error: null,
    refresh: vi.fn(),
    recentSessions: [],
    sortKey: "recent" as const,
    onSortKey: vi.fn(),
    onSelectDay: vi.fn(),
    onOpenSession: vi.fn(),
    onClearFilters: vi.fn(),
    ...over,
  };
}

describe("when the Explore landing renders with sessions", () => {
  it("should show the stats bar with real aggregates", () => {
    render(<ExploreDashboard {...props()} />);
    expect(screen.getByText("2")).toBeTruthy(); // session count
    expect(screen.getByText("512")).toBeTruthy(); // event count
    expect(screen.getByText(/busiest session/i)).toBeTruthy();
  });

  it("should render the calendar and forward a day click", () => {
    const p = props();
    const { container } = render(<ExploreDashboard {...p} />);
    const day = container.querySelector("[data-day='2026-06-10']") as HTMLElement;
    expect(day).toBeTruthy();
    fireEvent.click(day);
    expect(p.onSelectDay).toHaveBeenCalledWith("2026-06-10");
  });

  it("should open the busiest session on click", () => {
    const p = props();
    render(<ExploreDashboard {...p} />);
    fireEvent.click(screen.getByText(/busiest session/i).closest("button")!);
    expect(p.onOpenSession).toHaveBeenCalledWith("big-one");
  });
});

describe("when the sessions fetch fails", () => {
  it("should say what went wrong and offer a retry", () => {
    const p = props({ error: "network down", universe: [], stats: computeStats([]) });
    render(<ExploreDashboard {...p} />);
    expect(screen.getByText(/couldn't load sessions/i)).toBeTruthy();
    expect(screen.getByText("network down")).toBeTruthy();
    fireEvent.click(screen.getByText(/retry/i));
    expect(p.refresh).toHaveBeenCalled();
  });
});

describe("when filters match nothing", () => {
  it("should offer a reset instead of a dead end", () => {
    const p = props({ filtersActive: true, stats: computeStats([]) });
    render(<ExploreDashboard {...p} />);
    fireEvent.click(screen.getByText(/reset filters/i));
    expect(p.onClearFilters).toHaveBeenCalled();
  });
});
