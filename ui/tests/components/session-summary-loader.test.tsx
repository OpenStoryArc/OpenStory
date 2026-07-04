import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { SessionSummaryLoader, sessionSummaryCache } from "@/components/viz/SessionSummaryLoader";

/** The loader reads GET /api/sessions/{id}/summary (the ~600 B projection
 *  read), never the whole-session records. */
const SUMMARY = {
  session_id: "s1",
  status: "completed",
  start_time: "2026-06-30T10:00:00.000Z",
  last_event: "2026-06-30T10:00:30.000Z",
  duration_ms: null,
  event_count: 2,
  error_count: 0,
  tool_calls: 1,
  turn_count: 1,
  model: "claude-opus-4-8",
  tokens: { input: 10, output: 5, cache_creation: 0, cache_read: 0, total: 15 },
  top_files: [],
};

beforeEach(() => sessionSummaryCache.clear());
afterEach(() => vi.unstubAllGlobals());

describe("SessionSummaryLoader", () => {
  it("shows a skeleton while loading, then the summary header", async () => {
    let resolve!: (v: unknown) => void;
    vi.stubGlobal("fetch", vi.fn(() => new Promise((r) => { resolve = r; })));

    const { container } = render(<SessionSummaryLoader sessionId="s1" />);
    expect(screen.getByTestId("summary-loading")).toBeInTheDocument();

    resolve({ ok: true, json: async () => SUMMARY });
    await waitFor(() => expect(screen.getByText(/opus-4-8/)).toBeInTheDocument());
    expect(container).toHaveTextContent(/1 tool/i);
  });

  it("offers the climb to the spawner when the session is a subagent", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: true,
        json: async () => ({ ...SUMMARY, parent_session_id: "parent-123" }),
      })),
    );
    render(<SessionSummaryLoader sessionId="s1" />);
    await waitFor(() => expect(screen.getByTestId("parent-session-link")).toBeInTheDocument());
    expect(screen.getByTestId("parent-session-link").getAttribute("href")).toBe(
      "#/explore/parent-123",
    );
  });

  it("stays calm for a root session — no parent link", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, json: async () => SUMMARY })));
    render(<SessionSummaryLoader sessionId="s1" />);
    await waitFor(() => expect(screen.queryByTestId("summary-loading")).toBeNull());
    expect(screen.queryByTestId("parent-session-link")).toBeNull();
  });

  it("links '(n) failed' to the exact first-error event", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: true,
        json: async () => ({ ...SUMMARY, error_count: 2, first_error_event_id: "evt-boom" }),
      })),
    );
    render(<SessionSummaryLoader sessionId="s1" />);
    await waitFor(() => expect(screen.getByTestId("summary-errors")).toBeInTheDocument());
    expect(screen.getByTestId("summary-errors").getAttribute("href")).toBe(
      "#/explore/s1/event/evt-boom",
    );
  });

  it("renders nothing when the session has no events", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: true,
        json: async () => ({ ...SUMMARY, session_id: "empty", event_count: 0 }),
      })),
    );
    const { container } = render(<SessionSummaryLoader sessionId="empty" />);
    await waitFor(() => expect(screen.queryByTestId("summary-loading")).toBeNull());
    expect(container).toBeEmptyDOMElement();
  });
});
