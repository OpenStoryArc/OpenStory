import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { EventCardRow } from "@/components/events/EventCard";
import type { TimelineRow } from "@/lib/timeline";
import type { ViewRecord } from "@/types/view-record";

function makeRow(originAgent: string | null): TimelineRow {
  const record: ViewRecord = {
    id: "evt-1",
    seq: 1,
    session_id: "s1",
    timestamp: "2026-01-01T00:00:00Z",
    origin_agent: originAgent,
    record_type: "user_message",
    payload: {
      content: "hello",
      images: [],
    },
    agent_id: null,
    is_sidechain: false,
  };

  return {
    id: record.id,
    timestamp: record.timestamp,
    sessionId: record.session_id,
    category: "prompt",
    toolName: "",
    summary: "hello",
    record,
  };
}

describe("EventCardRow origin agent", () => {
  it("renders the origin agent badge", () => {
    render(<EventCardRow row={makeRow("pi-mono")} />);
    expect(screen.getByTestId("event-card-agent-badge").textContent).toBe("pi-mono");
  });

  it("omits the badge when the origin agent is unknown", () => {
    render(<EventCardRow row={makeRow(null)} />);
    expect(screen.queryByTestId("event-card-agent-badge")).toBeNull();
  });
});
