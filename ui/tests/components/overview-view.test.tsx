import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { OverviewView } from "@/components/overview/OverviewView";

/** Integration smoke test: mounts the whole dashboard against a mocked API and
 *  asserts it renders the stats bar, calendar, facets, and session list without
 *  crashing — the runtime confidence a headless browser would give, in jsdom. */

const SESSIONS = [
  {
    session_id: "big-one",
    start_time: "2026-06-10T09:00:00.000Z",
    last_event: "2026-06-10T12:00:00.000Z",
    event_count: 500,
    total_input_tokens: 4000,
    total_output_tokens: 6000,
    project_name: "OpenStory",
    branch: "master",
    status: "completed",
    host: "a1",
    user: "max",
    origin_agent: "claude-code",
    label: "build the dashboard",
  },
  {
    session_id: "small-one",
    start_time: "2026-06-11T14:00:00.000Z",
    last_event: "2026-06-11T14:05:00.000Z",
    event_count: 12,
    total_output_tokens: 100,
    project_name: "OpenStory",
    branch: "feat/x",
    status: "ongoing",
    host: "b2",
    user: "katie",
    origin_agent: "codex",
    label: "quick fix",
  },
];

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => ({
      ok: true,
      statusText: "OK",
      status: 200,
      json: async () => (String(url).includes("/api/sessions") && !String(url).includes("/records") ? { sessions: SESSIONS } : []),
    })),
  );
});

afterEach(() => vi.unstubAllGlobals());

describe("OverviewView (integration)", () => {
  it("renders stats, facets, and a row per session", async () => {
    render(<OverviewView route={{ view: "overview" }} onNavigate={() => {}} />);

    // stats bar: 2 sessions, 512 events total
    await waitFor(() => expect(screen.getByText("512")).toBeInTheDocument());
    expect(document.querySelectorAll("[data-session-row]")).toHaveLength(2);

    // busiest badge is on the 500-event session
    expect(screen.getByText("busiest")).toBeInTheDocument();

    // facet groups render their values
    expect(screen.getByText("codex")).toBeInTheDocument();
    expect(screen.getByText("katie")).toBeInTheDocument();
  });

  it("hydrates its filters from the URL route so a shared link restores the view", async () => {
    render(<OverviewView route={{ view: "overview", overview: { filters: { agent: "codex" } } }} onNavigate={() => {}} />);
    // only the codex session survives the URL-supplied filter
    await waitFor(() => expect(document.querySelectorAll("[data-session-row]")).toHaveLength(1));
    expect(document.querySelector('[data-session-row="small-one"]')).toBeInTheDocument();
  });

  it("filters the list when a facet is clicked", async () => {
    render(<OverviewView route={{ view: "overview" }} onNavigate={() => {}} />);
    await waitFor(() => expect(document.querySelectorAll("[data-session-row]")).toHaveLength(2));

    fireEvent.click(screen.getByText("codex"));
    await waitFor(() => expect(document.querySelectorAll("[data-session-row]")).toHaveLength(1));
    expect(document.querySelector('[data-session-row="small-one"]')).toBeInTheDocument();
  });

  it("re-sorts to put the busiest session first under 'Most events'", async () => {
    render(<OverviewView route={{ view: "overview" }} onNavigate={() => {}} />);
    await waitFor(() => expect(document.querySelectorAll("[data-session-row]")).toHaveLength(2));

    fireEvent.click(screen.getByText("Most events"));
    await waitFor(() => {
      const rows = document.querySelectorAll("[data-session-row]");
      expect(rows[0]?.getAttribute("data-session-row")).toBe("big-one");
    });
  });
});
