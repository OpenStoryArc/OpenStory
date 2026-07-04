/** Story's header renders from GET /api/sessions/{id}/summary (sub-ms,
 *  ~600 B) instead of the whole-session records fetch. This maps the API
 *  payload into the same SessionSummary the records fold produces, so one
 *  header component serves both sources. */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { summaryFromApi } from "@/lib/session-summary";

const API_PAYLOAD = {
  session_id: "sess-1",
  status: "completed",
  start_time: "2026-07-04T10:00:00Z",
  last_event: "2026-07-04T11:30:00Z",
  duration_ms: null,
  event_count: 19017,
  error_count: 3,
  tool_calls: 2305,
  turn_count: 163,
  model: "claude-opus-4-8",
  tokens: {
    input: 174681,
    output: 1939226,
    cache_creation: 6829664,
    cache_read: 713469890,
    total: 722413461,
  },
  top_files: [
    { path: "/src/a.tsx", count: 64 },
    { path: "/src/b.tsx", count: 50 },
  ],
};

describe("when the /summary payload carries the projection extras", () => {
  it("should map every stat the header renders", () =>
    scenario(
      () => API_PAYLOAD,
      (payload) => summaryFromApi(payload),
      (s) => {
        expect(s.eventCount).toBe(19017);
        expect(s.toolCount).toBe(2305);
        expect(s.errorCount).toBe(3);
        expect(s.turnCount).toBe(163);
        expect(s.model).toBe("claude-opus-4-8");
        expect(s.inputTokens).toBe(174681);
        expect(s.outputTokens).toBe(1939226);
        expect(s.cacheCreationTokens).toBe(6829664);
        expect(s.cacheReadTokens).toBe(713469890);
        expect(s.totalTokens).toBe(722413461);
        expect(s.topFiles).toEqual([
          { path: "/src/a.tsx", count: 64 },
          { path: "/src/b.tsx", count: 50 },
        ]);
        // duration_ms was null → derived from start_time→last_event (90 min)
        expect(s.durationMs).toBe(90 * 60 * 1000);
      },
    ));
});

describe("when the session is a spawned subagent", () => {
  it("should carry the parent session id (the climb to the spawner)", () =>
    scenario(
      () => ({ ...API_PAYLOAD, parent_session_id: "parent-123" }),
      (payload) => summaryFromApi(payload),
      (s) => expect(s.parentSessionId).toBe("parent-123"),
    ));

  it("should be null for a root session", () =>
    scenario(
      () => API_PAYLOAD,
      (payload) => summaryFromApi(payload),
      (s) => expect(s.parentSessionId).toBeNull(),
    ));
});

describe("when the payload has an explicit duration", () => {
  it("should prefer it over the timestamp span", () =>
    scenario(
      () => ({ ...API_PAYLOAD, duration_ms: 1234.0 }),
      (payload) => summaryFromApi(payload),
      (s) => expect(s.durationMs).toBe(1234),
    ));
});

describe("when the extras are absent (fallback path / older server)", () => {
  it("should degrade to zeros instead of crashing", () =>
    scenario(
      () => ({
        session_id: "sess-2",
        status: "completed",
        start_time: null,
        duration_ms: null,
        event_count: 5,
        error_count: 0,
        tool_calls: 2,
        model: null,
      }),
      (payload) => summaryFromApi(payload),
      (s) => {
        expect(s.eventCount).toBe(5);
        expect(s.turnCount).toBe(0);
        expect(s.totalTokens).toBe(0);
        expect(s.topFiles).toEqual([]);
        expect(s.durationMs).toBe(0);
        expect(s.model).toBeNull();
      },
    ));
});
