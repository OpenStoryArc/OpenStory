/** The SNAPPY requirement, proven at the component boundary:
 *  - surfaces that need the whole record array share ONE /records fetch
 *    through the record cache (no 91 MB × 2–4 refetch), and
 *  - the Story header (SessionSummaryLoader) doesn't fetch records at all —
 *    it renders from the ~600 B /summary projection read. */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { SessionSummaryLoader, sessionSummaryCache } from "@/components/viz/SessionSummaryLoader";
import { SessionTimeline } from "@/components/explore/SessionTimeline";
import { sessionRecordsCache } from "@/hooks/use-session-records";
import type { WireRecord } from "@/types/wire-record";

const SID = "shared-fetch-session";

function rec(partial: Partial<WireRecord> & { id: string; seq: number }): WireRecord {
  return {
    session_id: SID,
    record_type: "tool_call",
    timestamp: "2026-07-04T09:00:00Z",
    payload: { tool_name: "bash", typed_input: null },
    ...partial,
  } as unknown as WireRecord;
}

const RECORDS: WireRecord[] = [
  rec({ id: "e1", seq: 1 }),
  rec({
    id: "e2",
    seq: 2,
    record_type: "turn_end",
    payload: {},
  }),
];

const SUMMARY_PAYLOAD = {
  session_id: SID,
  status: "completed",
  start_time: "2026-07-04T09:00:00Z",
  last_event: "2026-07-04T09:30:00Z",
  duration_ms: null,
  event_count: 2,
  error_count: 0,
  tool_calls: 1,
  turn_count: 1,
  model: "claude-fable-5",
  tokens: { input: 10, output: 5, cache_creation: 0, cache_read: 0, total: 15 },
  top_files: [],
};

beforeEach(() => {
  sessionRecordsCache.clear();
  sessionSummaryCache.clear();
});

describe("when two record-consuming surfaces render the same session", () => {
  it("should hit /records exactly once and both render from the shared result", async () => {
    const fetchMock = vi.fn((url: string) => {
      expect(String(url)).toContain(`/api/sessions/${SID}/records`);
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve(RECORDS),
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(
      <div>
        <SessionTimeline sessionId={SID} />
        <SessionTimeline sessionId={SID} />
      </div>,
    );

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("should refetch after the cache is invalidated (a live session grew)", async () => {
    const fetchMock = vi.fn(() =>
      Promise.resolve({ ok: true, json: () => Promise.resolve(RECORDS) }),
    );
    vi.stubGlobal("fetch", fetchMock);

    render(<SessionTimeline sessionId={SID} />);
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));

    sessionRecordsCache.invalidate(SID);
    render(<SessionTimeline sessionId={SID} />);
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
  });
});

describe("when the Story header renders for a session", () => {
  it("should fetch /summary only — never the records", async () => {
    const urls: string[] = [];
    const fetchMock = vi.fn((url: string) => {
      urls.push(String(url));
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve(SUMMARY_PAYLOAD),
      });
    });
    vi.stubGlobal("fetch", fetchMock);

    render(<SessionSummaryLoader sessionId={SID} />);

    await waitFor(() => {
      expect(screen.queryByTestId("summary-loading")).toBeNull();
    });

    expect(urls).toEqual([`/api/sessions/${SID}/summary`]);
    // Renders real stats from the payload, not a blank strip.
    expect(screen.getByText("fable-5")).toBeTruthy();
    expect(screen.getByText("turn")).toBeTruthy();
  });
});
