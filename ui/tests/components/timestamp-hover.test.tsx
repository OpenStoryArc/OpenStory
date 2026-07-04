/** P4 polish: the WHEN is never ambiguous. Inline times stay compact
 *  (HH:MM:SS scan target); the full absolute stamp — time, zone, date — is
 *  one hover away everywhere a time renders. */

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { EventCardRow } from "@/components/events/EventCard";
import { SummaryStrip } from "@/components/viz/SessionSummaryHeader";
import { fullTimestamp } from "@/lib/time";
import type { TimelineRow } from "@/lib/timeline";
import type { SessionSummary } from "@/lib/session-summary";

const TS = "2026-07-04T10:00:00Z";

function row(): TimelineRow {
  return {
    category: "narrative",
    summary: "did the thing",
    timestamp: TS,
    toolName: undefined,
    record: {
      id: "evt-1",
      seq: 1,
      session_id: "s",
      timestamp: TS,
      record_type: "assistant_message",
      payload: { model: "m", content: [] },
      origin_agent: null,
      agent_id: null,
      is_sidechain: false,
    },
  } as unknown as TimelineRow;
}

function summary(): SessionSummary {
  return {
    eventCount: 5,
    toolCount: 2,
    errorCount: 0,
    turnCount: 1,
    startMs: Date.parse(TS),
    endMs: Date.parse(TS) + 90 * 60 * 1000,
    durationMs: 90 * 60 * 1000,
    inputTokens: 1,
    outputTokens: 1,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
    totalTokens: 2,
    model: "claude-fable-5",
    topFiles: [],
    parentSessionId: null,
    sessionId: "s",
    firstErrorEventId: null,
  };
}

describe("when an event card shows its compact time", () => {
  it("should carry the full absolute stamp as a hover title", () => {
    render(<EventCardRow row={row()} compact />);
    const el = screen.getByTestId("event-time");
    expect(el.getAttribute("title")).toBe(fullTimestamp(TS));
  });
});

describe("when the summary strip shows a duration", () => {
  it("should carry started→ended absolute stamps as a hover title", () => {
    render(<SummaryStrip summary={summary()} />);
    const el = screen.getByTestId("summary-duration");
    expect(el.getAttribute("title")).toContain(fullTimestamp(TS));
    expect(el.getAttribute("title")).toContain("→");
  });
});
