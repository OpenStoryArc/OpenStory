/** The event→turn canopy edge, human side: an expanded event card links to
 *  its turn in Story (#/story/SES/event/ID — StoryView scrolls to the turn
 *  containing the event). Compact rows stay calm — no link. */

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { EventCardRow } from "@/components/events/EventCard";
import type { TimelineRow } from "@/lib/timeline";

function row(): TimelineRow {
  return {
    category: "narrative",
    summary: "did the thing",
    timestamp: "2026-07-04T10:00:00Z",
    toolName: undefined,
    record: {
      id: "evt-42",
      seq: 7,
      session_id: "sess-story",
      timestamp: "2026-07-04T10:00:00Z",
      record_type: "assistant_message",
      payload: { model: "m", content: [] },
      origin_agent: null,
      agent_id: null,
      is_sidechain: false,
    },
  } as unknown as TimelineRow;
}

describe("when an event card is expanded", () => {
  it("should link to the event's turn in Story", () => {
    render(<EventCardRow row={row()} />);
    const link = screen.getByTestId("event-story-turn-link");
    expect(link.getAttribute("href")).toBe("#/story/sess-story/event/evt-42");
  });
});

describe("when the card is compact", () => {
  it("should keep the default calm — no story link", () => {
    render(<EventCardRow row={row()} compact />);
    expect(screen.queryByTestId("event-story-turn-link")).toBeNull();
  });
});
