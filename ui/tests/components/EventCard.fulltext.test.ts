import { describe, expect, it } from "vitest";
import { fullText } from "@/components/events/EventCard";
import type { ViewRecord } from "@/types/view-record";

/** A user prompt the way the BFF serializes it: `content` is a plain string,
 *  there is no flat `text` field, and no ContentBlock array. */
function userPrompt(content: string): ViewRecord {
  return {
    id: "evt-1",
    seq: 1,
    session_id: "s1",
    timestamp: "2026-01-01T00:00:00Z",
    origin_agent: null,
    record_type: "user_message",
    payload: { content, images: [] },
    agent_id: null,
    is_sidechain: false,
  } as unknown as ViewRecord;
}

describe("fullText (untruncated prompt extraction)", () => {
  it("returns the FULL string content for a user prompt, however long", () => {
    // Longer than MAX_SUMMARY (500) — the bug returned null here, forcing the
    // expanded card to fall back to the 500-char truncated summary.
    const long = "x".repeat(5000);
    expect(fullText(userPrompt(long))).toBe(long);
  });

  it("still extracts text from a ContentBlock array (assistant messages)", () => {
    const record = {
      id: "evt-2",
      seq: 2,
      session_id: "s1",
      timestamp: "2026-01-01T00:00:00Z",
      origin_agent: null,
      record_type: "assistant_message",
      payload: { content: [{ type: "text", text: "block text" }] },
      agent_id: null,
      is_sidechain: false,
    } as unknown as ViewRecord;
    expect(fullText(record)).toBe("block text");
  });

  it("returns null when there is genuinely no text", () => {
    const record = {
      id: "evt-3",
      seq: 3,
      session_id: "s1",
      timestamp: "2026-01-01T00:00:00Z",
      origin_agent: null,
      record_type: "system_event",
      payload: { subtype: "turn.complete" },
      agent_id: null,
      is_sidechain: false,
    } as unknown as ViewRecord;
    expect(fullText(record)).toBeNull();
  });
});
