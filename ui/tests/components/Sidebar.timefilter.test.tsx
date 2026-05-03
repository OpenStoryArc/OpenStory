/**
 * Spec: <Sidebar> integration with the URL-driven timeFilter prop.
 *
 * The pure helpers (`timeFilterMatches`) and the presentational pill row
 * (`<TimeFilter>`) are covered elsewhere. This file covers the wiring
 * in between — the `filteredSessions` memo, the count-chip, and the
 * "no matches" empty state — by mounting the real component against a
 * stubbed `/api/sessions` and asserting on the rendered DOM.
 *
 * Time is frozen with `vi.setSystemTime()` so the boundary computations
 * inside `timeFilterMatches` (today = midnight local, week = previous
 * Sunday) line up with the fixture timestamps no matter what wall clock
 * CI happens to be running on.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { Sidebar } from "@/components/Sidebar";
import type { StorySession } from "@/lib/story-api";

// Wed 2026-05-06 14:30 local — matches the anchor used by the
// `time-filter` unit spec so the two tests share intuition about which
// timestamps fall inside which window.
function anchorNow(): Date {
  const d = new Date();
  d.setFullYear(2026, 4, 6); // May (0-indexed)
  d.setHours(14, 30, 0, 0);
  return d;
}

function isoMinusMinutes(n: number): string {
  return new Date(anchorNow().getTime() - n * 60_000).toISOString();
}

function isoMinusHours(n: number): string {
  return isoMinusMinutes(n * 60);
}

function isoMinusDays(n: number): string {
  return isoMinusMinutes(n * 24 * 60);
}

function session(overrides: Partial<StorySession> & { session_id: string; last_event: string }): StorySession {
  const { session_id, last_event, ...rest } = overrides;
  return {
    session_id,
    last_event,
    label: `Session ${session_id}`,
    branch: "main",
    event_count: 5,
    start_time: last_event,
    host: null,
    user: null,
    total_input_tokens: 0,
    total_output_tokens: 0,
    ...rest,
  };
}

/**
 * Stub `fetch` so it returns sensible payloads for each endpoint the
 * Sidebar (and its child PersonRow) reach for. Returning 0 users makes
 * PersonRow self-hide, keeping these specs focused on session rows.
 */
function stubApi(sessions: StorySession[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = typeof input === "string" ? input : input.toString();
      if (url.includes("/api/users")) {
        return new Response(JSON.stringify({ users: [], total: 0 }), { status: 200 });
      }
      if (url.includes("/api/sessions")) {
        return new Response(
          JSON.stringify({ sessions, total: sessions.length }),
          { status: 200 },
        );
      }
      return new Response("not found", { status: 404 });
    }),
  );
}

const noop = () => {};

const baseProps = {
  events: [],
  selectedSession: null,
  onSelectSession: noop,
  focusAgentId: null,
  onFocusAgent: noop,
} as const;

describe("<Sidebar> + timeFilter", () => {
  beforeEach(() => {
    // Fake only Date — leave the microtask/timer queues real so fetch()
    // and React's useEffect cleanup still resolve. `vi.useFakeTimers()`
    // with the default profile freezes promise resolution and the whole
    // suite times out.
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(anchorNow());
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  // 4 sessions placed at known offsets from `anchorNow`:
  //   recent  — 1 minute ago     → in 1h, today, week, all
  //   midday  — 3 hours ago      → in       today, week, all
  //   monday  — 2 days  ago      → in              week, all
  //   ancient — 30 days ago      → in                    all
  const fixtureSessions: StorySession[] = [
    session({ session_id: "s-recent",  last_event: isoMinusMinutes(1),  user: "katie" }),
    session({ session_id: "s-midday",  last_event: isoMinusHours(3),    user: "max" }),
    session({ session_id: "s-monday",  last_event: isoMinusDays(2),     user: "katie" }),
    session({ session_id: "s-ancient", last_event: isoMinusDays(30),    user: "max" }),
  ];

  async function renderSidebar(props: Partial<React.ComponentProps<typeof Sidebar>>) {
    stubApi(fixtureSessions);
    render(<Sidebar {...baseProps} {...props} />);
    // Wait for the REST fetch + first render to settle. testids come
    // from `s.id.slice(0, 8)` in Sidebar.tsx.
    await waitFor(() => {
      expect(screen.getByTestId("session-s-recent")).toBeTruthy();
    });
  }

  describe("filteredSessions memo", () => {
    it("renders every session when timeFilter is 'all'", async () => {
      await renderSidebar({ timeFilter: "all" });
      expect(screen.getByTestId("session-s-recent")).toBeTruthy();
      expect(screen.getByTestId("session-s-midday")).toBeTruthy();
      expect(screen.getByTestId("session-s-monday")).toBeTruthy();
      expect(screen.getByTestId("session-s-ancien")).toBeTruthy();
    });

    it("hides sessions older than 1h when timeFilter is '1h'", async () => {
      await renderSidebar({ timeFilter: "1h" });
      expect(screen.getByTestId("session-s-recent")).toBeTruthy();
      expect(screen.queryByTestId("session-s-midday")).toBeNull();
      expect(screen.queryByTestId("session-s-monday")).toBeNull();
      expect(screen.queryByTestId("session-s-ancien")).toBeNull();
    });

    it("hides sessions before midnight today when timeFilter is 'today'", async () => {
      await renderSidebar({ timeFilter: "today" });
      expect(screen.getByTestId("session-s-recent")).toBeTruthy();
      expect(screen.getByTestId("session-s-midday")).toBeTruthy();
      expect(screen.queryByTestId("session-s-monday")).toBeNull();
      expect(screen.queryByTestId("session-s-ancien")).toBeNull();
    });

    it("hides sessions before last Sunday when timeFilter is 'week'", async () => {
      // Anchor is Wed 5/6 → previous Sunday boundary is 5/3 00:00 local.
      // Mon 5/4 14:30 is inside the window; 30 days ago is not.
      await renderSidebar({ timeFilter: "week" });
      expect(screen.getByTestId("session-s-recent")).toBeTruthy();
      expect(screen.getByTestId("session-s-midday")).toBeTruthy();
      expect(screen.getByTestId("session-s-monday")).toBeTruthy();
      expect(screen.queryByTestId("session-s-ancien")).toBeNull();
    });

    it("composes timeFilter with userFilter (logical AND)", async () => {
      // userFilter=katie keeps recent + monday; timeFilter=today drops monday.
      await renderSidebar({ timeFilter: "today", userFilter: "katie" });
      expect(screen.getByTestId("session-s-recent")).toBeTruthy();
      expect(screen.queryByTestId("session-s-midday")).toBeNull(); // wrong user
      expect(screen.queryByTestId("session-s-monday")).toBeNull(); // wrong window
      expect(screen.queryByTestId("session-s-ancien")).toBeNull();
    });
  });

  describe("session-count chip", () => {
    it("shows just the total when no filter is active", async () => {
      await renderSidebar({ timeFilter: "all" });
      expect(screen.getByTestId("sidebar-session-count").textContent).toBe("4");
    });

    it("shows X / Y when timeFilter alone narrows the list", async () => {
      // Regression guard: previously the chip only swapped to "X / Y"
      // when host or user was set, so a standalone time filter silently
      // displayed the unfiltered total.
      await renderSidebar({ timeFilter: "1h" });
      expect(screen.getByTestId("sidebar-session-count").textContent).toMatch(/^\s*1\s*\/\s*4\s*$/);
    });

    it("shows X / Y when userFilter and timeFilter combine", async () => {
      await renderSidebar({ timeFilter: "today", userFilter: "katie" });
      expect(screen.getByTestId("sidebar-session-count").textContent).toMatch(/^\s*1\s*\/\s*4\s*$/);
    });
  });

  describe("no-matches empty state", () => {
    it("appears when timeFilter alone hides every session", async () => {
      // Push the entire fixture out of the 1h window.
      stubApi([
        session({ session_id: "stale-1", last_event: isoMinusHours(5) }),
        session({ session_id: "stale-2", last_event: isoMinusDays(2) }),
      ]);
      render(<Sidebar {...baseProps} timeFilter="1h" />);
      await waitFor(() => {
        // Empty-state container should render once data has loaded.
        expect(screen.getByTestId("sidebar-no-matches")).toBeTruthy();
      });
      // And it should give the user a way out.
      expect(screen.getByTestId("sidebar-clear-filters")).toBeTruthy();
    });

    it("Clear filters resets timeFilter back to 'all'", async () => {
      const onTimeFilterChange = vi.fn();
      stubApi([session({ session_id: "stale-1", last_event: isoMinusHours(5) })]);
      render(
        <Sidebar
          {...baseProps}
          timeFilter="1h"
          onTimeFilterChange={onTimeFilterChange}
        />,
      );
      await waitFor(() => {
        expect(screen.getByTestId("sidebar-clear-filters")).toBeTruthy();
      });
      fireEvent.click(screen.getByTestId("sidebar-clear-filters"));
      expect(onTimeFilterChange).toHaveBeenCalledWith("all");
    });
  });
});
