/** The plan→turn edge, human side: a selected plan links to the Story turn
 *  that authored it (via its ExitPlanMode event). No authoring event found →
 *  no link, never a wrong guess. */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { PlanViewer } from "@/components/plans/PlanViewer";
import { sessionRecordsCache } from "@/hooks/use-session-records";

const PLAN = {
  id: "p1",
  session_id: "sess-p",
  title: "Fix the auth bug",
  timestamp: "2026-07-04T10:01:00Z",
  content: "# Fix the auth bug\n1. do it",
};

const EXIT_RECORD = {
  id: "evt-exit",
  seq: 3,
  session_id: "sess-p",
  timestamp: "2026-07-04T10:01:00Z",
  record_type: "tool_call",
  payload: { name: "ExitPlanMode", raw_input: { plan: "# Fix the auth bug\n1. do it" }, input: {}, call_id: "c1" },
};

beforeEach(() => sessionRecordsCache.clear());
afterEach(() => vi.unstubAllGlobals());

function stubFetch(records: unknown[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string) => {
      const u = String(url);
      const body = u.includes("/records")
        ? records
        : u.endsWith("/plans")
          ? [PLAN] // list endpoints return a bare array
          : PLAN; // /api/plans/{id} detail
      return Promise.resolve({ ok: true, json: async () => body });
    }),
  );
}

describe("when a selected plan's authoring event is known", () => {
  it("should link to its turn in Story", async () => {
    stubFetch([EXIT_RECORD]);
    render(<PlanViewer sessionId="sess-p" initialPlanId="p1" />);
    await waitFor(() => expect(screen.getByTestId("plan-turn-link")).toBeInTheDocument());
    expect(screen.getByTestId("plan-turn-link").getAttribute("href")).toBe(
      "#/story/sess-p/event/evt-exit",
    );
  });
});

describe("when no authoring event exists in the session", () => {
  it("should stay calm — no link, no wrong guess", async () => {
    stubFetch([]);
    render(<PlanViewer sessionId="sess-p" initialPlanId="p1" />);
    await waitFor(() => expect(screen.getByText(/do it/)).toBeInTheDocument());
    expect(screen.queryByTestId("plan-turn-link")).toBeNull();
  });
});
