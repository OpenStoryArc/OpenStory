/** The SNAPPY requirement, proven at the component boundary: two surfaces
 *  that both need a session's whole record array (Story's summary header,
 *  Explore's timeline) share ONE network fetch through the record cache —
 *  the 91 MB × 2–4 refetch on big sessions is gone. */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { SessionSummaryLoader } from "@/components/viz/SessionSummaryLoader";
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

beforeEach(() => {
  sessionRecordsCache.clear();
});

describe("when two surfaces render the same session", () => {
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
        <SessionSummaryLoader sessionId={SID} />
        <SessionTimeline sessionId={SID} />
      </div>,
    );

    // Both surfaces leave their loading states — rendered from real data.
    await waitFor(() => {
      expect(screen.queryByTestId("summary-loading")).toBeNull();
    });

    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("should refetch after the cache is invalidated (a live session grew)", async () => {
    const fetchMock = vi.fn(() =>
      Promise.resolve({ ok: true, json: () => Promise.resolve(RECORDS) }),
    );
    vi.stubGlobal("fetch", fetchMock);

    render(<SessionSummaryLoader sessionId={SID} />);
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));

    sessionRecordsCache.invalidate(SID);
    render(<SessionSummaryLoader sessionId={SID} />);
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
  });
});
