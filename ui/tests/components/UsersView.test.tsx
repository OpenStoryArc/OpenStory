/**
 * Spec: UsersView — top-level Users tab rendering.
 *
 * Asserts the load → render path produces the expected portrait per user:
 * project chips, host badges, recent sessions, and the activity sparkline.
 * Click handlers navigate via the supplied prop.
 *
 * UserCard is internal to UsersView; we test it through the parent so
 * the integration is what's pinned.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { UsersView } from "@/components/users/UsersView";
import type { UsersResponse } from "@/lib/users-api";

function userPayload(overrides: Partial<UsersResponse["users"][0]> = {}) {
  return {
    user: "katie",
    session_count: 32,
    hosts: ["Katies-Mac-mini"],
    projects: ["OpenStory", "openstory-ui-prototype"],
    last_active: "2026-05-02T17:00:00Z",
    total_input_tokens: 12345,
    total_output_tokens: 67890,
    activity_24h: Array(24).fill(0).map((_, i) => i),
    recent_sessions: [
      {
        session_id: "abcdef12-3456-7890-abcd-ef1234567890",
        label: "fix the migration",
        last_event: "2026-05-02T17:00:00Z",
        project_name: "OpenStory",
        event_count: 87,
      },
    ],
    ...overrides,
  };
}

function stubFetch(payload: UsersResponse, status = 200) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () =>
      new Response(JSON.stringify(payload), { status }),
    ),
  );
}

describe("UsersView", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders the loading state initially", () => {
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
    render(<UsersView onNavigate={() => {}} />);
    expect(screen.getByText(/Loading users/)).toBeTruthy();
  });

  it("renders the welcome empty state when no sessions on disk at all", async () => {
    stubFetch({ users: [], total: 0 });
    render(<UsersView onNavigate={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId("users-empty")).toBeTruthy();
    });
    expect(screen.getByText(/Welcome to OpenStory/)).toBeTruthy();
    // The "Run an agent" copy belongs to the no-data branch only.
    expect(screen.getByText(/Run an agent/)).toBeTruthy();
  });

  it("renders the unstamped-only empty state with onboarding hint when sessions exist but none are stamped", async () => {
    stubFetch({ users: [], total: 5 });
    render(<UsersView onNavigate={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId("users-empty")).toBeTruthy();
    });
    expect(screen.getByText(/No stamped users yet/)).toBeTruthy();
    expect(screen.getByText(/OPEN_STORY_USER/)).toBeTruthy();
    // "5 sessions on disk, but none have a stamped user."
    expect(screen.getByTestId("users-empty").textContent).toMatch(/5 sessions? on disk/);
  });

  it("renders the failure state on a non-200 response", async () => {
    stubFetch({ users: [], total: 0 }, 500);
    render(<UsersView onNavigate={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText(/Failed to load users/)).toBeTruthy();
    });
  });

  it("renders a user card with @user, host badge, project chips, and sparkline", async () => {
    stubFetch({ users: [userPayload()], total: 1 });
    render(<UsersView onNavigate={() => {}} />);

    await waitFor(() => {
      expect(screen.getByTestId("user-card-katie")).toBeTruthy();
    });
    expect(screen.getByText("@katie")).toBeTruthy();
    expect(screen.getByText("32 sessions")).toBeTruthy();
    expect(screen.getByText(/⌂ Katies-Mac-mini/)).toBeTruthy();
    // Project chips replace the comma list — both projects rendered as
    // bordered chips with a `title=` attribute.
    expect(screen.getByTitle("Project: OpenStory")).toBeTruthy();
    expect(screen.getByTitle("Project: openstory-ui-prototype")).toBeTruthy();
    // Sparkline is inside the card.
    const card = screen.getByTestId("user-card-katie");
    expect(card.querySelector('[data-testid="activity-sparkline"]')).toBeTruthy();
  });

  it("renders the recent_sessions list with project_name and event count", async () => {
    stubFetch({ users: [userPayload()], total: 1 });
    render(<UsersView onNavigate={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText(/fix the migration/)).toBeTruthy();
    });
    expect(screen.getByText(/87 events/)).toBeTruthy();
  });

  it("clicking a recent session navigates to Live with that session_id", async () => {
    const onNavigate = vi.fn();
    stubFetch({ users: [userPayload()], total: 1 });
    render(<UsersView onNavigate={onNavigate} />);

    await waitFor(() => {
      expect(
        screen.getByTestId("user-card-katie-session-abcdef12"),
      ).toBeTruthy();
    });
    fireEvent.click(screen.getByTestId("user-card-katie-session-abcdef12"));
    expect(onNavigate).toHaveBeenCalledWith({
      view: "live",
      sessionId: "abcdef12-3456-7890-abcd-ef1234567890",
    });
  });

  it("caps project chips at 4 with a +N overflow", async () => {
    stubFetch({
      users: [
        userPayload({
          projects: ["a", "b", "c", "d", "e", "f", "g"],
        }),
      ],
      total: 1,
    });
    render(<UsersView onNavigate={() => {}} />);

    await waitFor(() => {
      expect(screen.getByTestId("user-card-katie")).toBeTruthy();
    });
    // 4 chips visible, then "+3"
    expect(screen.getByTitle("Project: a")).toBeTruthy();
    expect(screen.getByTitle("Project: b")).toBeTruthy();
    expect(screen.getByTitle("Project: c")).toBeTruthy();
    expect(screen.getByTitle("Project: d")).toBeTruthy();
    expect(screen.queryByTitle("Project: e")).toBeNull();
    expect(screen.getByText("+3")).toBeTruthy();
  });

  it("hides the projects row entirely when the list is empty", async () => {
    stubFetch({ users: [userPayload({ projects: [] })], total: 1 });
    render(<UsersView onNavigate={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId("user-card-katie")).toBeTruthy();
    });
    expect(screen.queryByText(/^projects:$/)).toBeNull();
  });
});
