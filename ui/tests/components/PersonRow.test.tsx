/**
 * Spec: PersonRow — fetches /api/users, renders chips, hides on ≤1 user,
 * toggles the user filter on click, "All" clears.
 *
 * `fetchUsers` is mocked at the module boundary so the component's
 * data-fetching path runs without a real server.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { PersonRow } from "@/components/PersonRow";
import type { UsersResponse } from "@/lib/users-api";

function userSummary(overrides: Partial<UsersResponse["users"][0]> = {}) {
  return {
    user: "katie",
    session_count: 32,
    hosts: ["Katies-Mac-mini"],
    projects: ["OpenStory"],
    last_active: new Date(Date.now() - 60_000).toISOString(), // 1 min ago
    total_input_tokens: 0,
    total_output_tokens: 0,
    activity_24h: Array(24).fill(0),
    recent_sessions: [],
    ...overrides,
  };
}

function stubFetch(users: UsersResponse["users"]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () =>
      new Response(JSON.stringify({ users, total: users.length }), {
        status: 200,
      }),
    ),
  );
}

describe("PersonRow", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders nothing while loading", () => {
    // fetch returns a never-resolving promise → component stays in loading
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
    const { container } = render(
      <PersonRow userFilter={null} onUserFilterChange={() => {}} />,
    );
    expect(container.querySelector('[data-testid="person-row"]')).toBeNull();
  });

  it("renders nothing when there are 0 stamped users", async () => {
    stubFetch([]);
    const { container } = render(
      <PersonRow userFilter={null} onUserFilterChange={() => {}} />,
    );
    await waitFor(() => {
      expect(globalThis.fetch).toHaveBeenCalled();
    });
    expect(container.querySelector('[data-testid="person-row"]')).toBeNull();
  });

  it("renders nothing when there is exactly 1 stamped user (filter of one is noise)", async () => {
    stubFetch([userSummary({ user: "katie" })]);
    const { container } = render(
      <PersonRow userFilter={null} onUserFilterChange={() => {}} />,
    );
    await waitFor(() => {
      expect(globalThis.fetch).toHaveBeenCalled();
    });
    expect(container.querySelector('[data-testid="person-row"]')).toBeNull();
  });

  it("renders the row with All + each user when there are >=2 users", async () => {
    stubFetch([
      userSummary({ user: "katie", session_count: 32 }),
      userSummary({ user: "maxglassie", session_count: 202, last_active: null }),
    ]);
    render(<PersonRow userFilter={null} onUserFilterChange={() => {}} />);

    await waitFor(() => {
      expect(screen.getByTestId("person-row")).toBeTruthy();
    });
    expect(screen.getByTestId("person-chip-all")).toBeTruthy();
    expect(screen.getByTestId("person-chip-katie")).toBeTruthy();
    expect(screen.getByTestId("person-chip-maxglassie")).toBeTruthy();
    // "All (2)" shows the user count.
    expect(screen.getByTestId("person-chip-all").textContent).toMatch(/All \(2\)/);
  });

  it("clicking a chip sets the user filter to that user", async () => {
    const onChange = vi.fn();
    stubFetch([
      userSummary({ user: "katie" }),
      userSummary({ user: "maxglassie" }),
    ]);
    render(<PersonRow userFilter={null} onUserFilterChange={onChange} />);

    await waitFor(() => {
      expect(screen.getByTestId("person-chip-maxglassie")).toBeTruthy();
    });
    fireEvent.click(screen.getByTestId("person-chip-maxglassie"));
    expect(onChange).toHaveBeenCalledWith("maxglassie");
  });

  it("clicking the currently-selected chip clears the filter", async () => {
    const onChange = vi.fn();
    stubFetch([
      userSummary({ user: "katie" }),
      userSummary({ user: "maxglassie" }),
    ]);
    render(<PersonRow userFilter="katie" onUserFilterChange={onChange} />);

    await waitFor(() => {
      expect(screen.getByTestId("person-chip-katie")).toBeTruthy();
    });
    fireEvent.click(screen.getByTestId("person-chip-katie"));
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("clicking 'All' clears the filter", async () => {
    const onChange = vi.fn();
    stubFetch([
      userSummary({ user: "katie" }),
      userSummary({ user: "maxglassie" }),
    ]);
    render(<PersonRow userFilter="katie" onUserFilterChange={onChange} />);

    await waitFor(() => {
      expect(screen.getByTestId("person-chip-all")).toBeTruthy();
    });
    fireEvent.click(screen.getByTestId("person-chip-all"));
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("the All chip reflects whether a filter is active", async () => {
    stubFetch([userSummary({ user: "a" }), userSummary({ user: "b" })]);
    const { rerender } = render(
      <PersonRow userFilter={null} onUserFilterChange={() => {}} />,
    );
    await waitFor(() => {
      expect(screen.getByTestId("person-chip-all").getAttribute("data-selected")).toBe("true");
    });
    rerender(<PersonRow userFilter="a" onUserFilterChange={() => {}} />);
    expect(screen.getByTestId("person-chip-all").getAttribute("data-selected")).toBe("false");
  });
});
