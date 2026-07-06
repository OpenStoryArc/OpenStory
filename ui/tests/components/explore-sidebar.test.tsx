import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { useState } from "react";
import { ExploreSidebar } from "@/components/explore/ExploreSidebar";
import type { OverviewFilters, SortKey } from "@/lib/sessions-overview";
import type { StorySession } from "@/lib/story-api";

/** The merged sidebar: ONE URL-owned filter model (facets + range + search +
 *  sort) driving the parent/subagent hierarchy list. Controlled component —
 *  filters live in ExploreView and round-trip through the URL. */

const NOW = new Date("2026-06-12T12:00:00.000Z").getTime();

const SESSIONS: StorySession[] = [
  {
    session_id: "recent-openstory",
    start_time: "2026-06-11T09:00:00.000Z",
    last_event: "2026-06-11T12:00:00.000Z",
    event_count: 500,
    project_id: "p-openstory",
    project_name: "OpenStory",
    status: "completed",
    user: "max",
    label: "build the dashboard",
  },
  {
    session_id: "agent-child",
    start_time: "2026-06-11T09:05:00.000Z",
    last_event: "2026-06-11T09:30:00.000Z",
    event_count: 40,
    project_id: "p-openstory",
    project_name: "OpenStory",
    status: "completed",
    user: "max",
  },
  {
    session_id: "old-otherproj",
    start_time: "2026-05-20T09:00:00.000Z",
    last_event: "2026-05-20T10:00:00.000Z",
    event_count: 90,
    project_id: "p-other",
    project_name: "OtherProj",
    status: "ongoing",
    user: "katie",
    label: "old experiment",
  },
];

/** Stateful harness — mimics ExploreView owning the filter state. */
function Harness({ onSelect = vi.fn() }: { onSelect?: (id: string) => void }) {
  const [filters, setFilters] = useState<OverviewFilters>({});
  const [sortKey, setSortKey] = useState<SortKey>("recent");
  return (
    <ExploreSidebar
      sessions={SESSIONS}
      loading={false}
      filters={filters}
      sortKey={sortKey}
      nowMs={NOW}
      onFiltersChange={setFilters}
      onSortChange={setSortKey}
      selectedSessionId={null}
      onSelectSession={onSelect}
    />
  );
}

describe("facet disclosure", () => {
  it("should keep facets collapsed by default so the session list stays above the fold", () => {
    render(<Harness />);
    expect(screen.queryByTestId("facet-user-katie")).toBeNull();
    fireEvent.click(screen.getByTestId("facets-toggle"));
    expect(screen.getByTestId("facet-user-katie")).toBeTruthy();
  });
});

describe("when a facet is clicked", () => {
  it("should narrow the session list to matching parents", () => {
    render(<Harness />);
    // Both parents visible initially.
    expect(screen.getByTestId("explore-session-recent-openstory")).toBeTruthy();
    expect(screen.getByTestId("explore-session-old-otherproj")).toBeTruthy();
    // Facet: user "katie" (facet rows carry data-facet testids).
    fireEvent.click(screen.getByTestId("facets-toggle"));
    fireEvent.click(screen.getByTestId("facet-user-katie"));
    expect(screen.queryByTestId("explore-session-recent-openstory")).toBeNull();
    expect(screen.getByTestId("explore-session-old-otherproj")).toBeTruthy();
  });
});

describe("when a date-range chip is set", () => {
  it("should hide sessions whose last activity is outside the window", () => {
    render(<Harness />);
    fireEvent.click(screen.getByTestId("date-range-7d"));
    expect(screen.getByTestId("explore-session-recent-openstory")).toBeTruthy();
    expect(screen.queryByTestId("explore-session-old-otherproj")).toBeNull();
  });
});

describe("when filters are active", () => {
  it("should clear everything with one click", () => {
    render(<Harness />);
    fireEvent.click(screen.getByTestId("date-range-7d"));
    expect(screen.queryByTestId("explore-session-old-otherproj")).toBeNull();
    fireEvent.click(screen.getByText(/clear all filters/i));
    expect(screen.getByTestId("explore-session-old-otherproj")).toBeTruthy();
  });
});

describe("when the sort chips change", () => {
  it("should reorder parents (most events first)", () => {
    render(<Harness />);
    fireEvent.click(screen.getByText("Most events"));
    const rows = screen.getAllByTestId(/^explore-session-/);
    expect(rows[0]!.getAttribute("data-testid")).toBe("explore-session-recent-openstory");
  });
});

describe("when searching", () => {
  it("should match by label through the shared filter model", () => {
    render(<Harness />);
    fireEvent.change(screen.getByTestId("explore-search"), { target: { value: "experiment" } });
    expect(screen.queryByTestId("explore-session-recent-openstory")).toBeNull();
    expect(screen.getByTestId("explore-session-old-otherproj")).toBeTruthy();
  });
});

describe("subagent hierarchy", () => {
  it("should nest agent sessions under their parent, not list them top-level", () => {
    render(<Harness />);
    expect(screen.queryByTestId("explore-session-agent-child")).toBeNull();
    expect(screen.getByText(/1 subagent/)).toBeTruthy();
  });
});
