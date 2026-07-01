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
    vi.fn(async (url: string) => {
      const u = String(url);
      const body = () => {
        // the session list (no id-scoped suffix)
        if (/\/api\/sessions(\?|$)/.test(u)) return { sessions: SESSIONS };
        // drill-in detail endpoints — object for synopsis, arrays for the rest
        if (u.includes("/synopsis")) return null;
        // records + file-impact / errors / tool-journey
        return [];
      };
      return { ok: true, statusText: "OK", status: 200, json: async () => body() };
    }),
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

  it("shows a layout-matched skeleton while the session list is loading", () => {
    // fetch that never resolves → stays in the loading state
    vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
    render(<OverviewView route={{ view: "overview" }} onNavigate={() => {}} />);
    expect(screen.getByTestId("session-list-skeleton")).toBeInTheDocument();
    expect(document.querySelectorAll("[data-session-row]")).toHaveLength(0);
  });

  it("offers an inline recovery action when filters match nothing", async () => {
    render(
      <OverviewView route={{ view: "overview", overview: { filters: { user: "nobody-here" } } }} onNavigate={() => {}} />,
    );
    await waitFor(() => expect(screen.getByText(/no sessions match/i)).toBeInTheDocument());
    expect(document.querySelectorAll("[data-session-row]")).toHaveLength(0);
    // recovering clears filters and the sessions come back
    fireEvent.click(screen.getByText("Reset filters"));
    await waitFor(() => expect(document.querySelectorAll("[data-session-row]").length).toBeGreaterThan(0));
  });

  it("navigates the list with j/k and opens the highlighted session on Enter", async () => {
    render(<OverviewView route={{ view: "overview" }} onNavigate={() => {}} />);
    await waitFor(() => expect(document.querySelectorAll("[data-session-row]")).toHaveLength(2));

    const list = screen.getByLabelText(/session list/i);
    fireEvent.keyDown(list, { key: "j" }); // → row 0
    fireEvent.keyDown(list, { key: "j" }); // → row 1
    const rows = document.querySelectorAll("[data-session-row]");
    expect(rows[1]?.getAttribute("data-highlighted")).toBe("true");
    expect(rows[0]?.getAttribute("data-highlighted")).toBeNull();

    // k moves back up to row 0
    fireEvent.keyDown(list, { key: "k" });
    expect(document.querySelectorAll("[data-session-row]")[0]?.getAttribute("data-highlighted")).toBe("true");

    // Enter opens the highlighted session's drill-in
    fireEvent.keyDown(list, { key: "Enter" });
    await waitFor(() => expect(screen.getByText(/open in explore/i)).toBeInTheDocument());
  });

  it("shows an honest error state (not 'no sessions') when the fetch fails", async () => {
    const fetchMock = vi.fn(() => Promise.reject(new Error("network down")));
    vi.stubGlobal("fetch", fetchMock);

    render(<OverviewView route={{ view: "overview" }} onNavigate={() => {}} />);
    await waitFor(() => expect(screen.getByTestId("overview-error")).toBeInTheDocument());
    // must NOT masquerade as an empty result
    expect(screen.queryByText(/no sessions match/i)).toBeNull();
    expect(screen.getByText(/network down/i)).toBeInTheDocument();

    // Retry re-fetches
    const before = fetchMock.mock.calls.length;
    fireEvent.click(screen.getByText(/retry/i));
    await waitFor(() => expect(fetchMock.mock.calls.length).toBeGreaterThan(before));
  });

  it("shows a Recent strip of previously-opened sessions", async () => {
    // seed frecency: 'small-one' was recently viewed
    window.localStorage.setItem(
      "openstory.recents.v1",
      JSON.stringify({ entries: [{ id: "small-one", count: 1, lastVisit: 1_000_000 }] }),
    );
    render(<OverviewView route={{ view: "overview" }} onNavigate={() => {}} />);
    await waitFor(() => expect(screen.getByTestId("recent-strip")).toBeInTheDocument());
    expect(document.querySelector('[data-recent-session="small-one"]')).toBeInTheDocument();
    window.localStorage.clear();
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
