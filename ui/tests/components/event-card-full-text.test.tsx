/** Regression: a user_message whose payload.content is a plain STRING (not a
 *  content-block array) must render its full text — not fall back to the
 *  1,200-char clamped row.summary from timeline.ts (MAX_SUMMARY). Found via
 *  dogfooding: event 1898d6a5… stored all 27,899 chars, the card showed ~1,200
 *  and an ellipsis because fullText() only handled the array shape. */

import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { EventCardRow } from "@/components/events/EventCard";
import type { TimelineRow } from "@/lib/timeline";

const FULL_TEXT =
  "The start of a very long prompt. " + "middle filler text. ".repeat(80) + "THE-FULL-TAIL-MARKER";
const CLAMPED = FULL_TEXT.slice(0, 1200) + "…";

function stringContentRow(): TimelineRow {
  return {
    category: "narrative",
    summary: CLAMPED,
    timestamp: "2026-08-04T10:00:00Z",
    toolName: undefined,
    record: {
      id: "evt-full-text",
      seq: 281,
      session_id: "sess-clamp",
      timestamp: "2026-08-04T10:00:00Z",
      record_type: "user_message",
      payload: { content: FULL_TEXT },
      origin_agent: null,
      agent_id: null,
      is_sidechain: false,
    },
  } as unknown as TimelineRow;
}

describe("when a user_message stores content as a plain string", () => {
  it("should render the full text, not the clamped summary", () => {
    const { container } = render(<EventCardRow row={stringContentRow()} />);
    expect(container.textContent).toContain("THE-FULL-TAIL-MARKER");
    expect(container.textContent).not.toContain("…");
  });
});
