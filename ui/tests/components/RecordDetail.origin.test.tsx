import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RecordDetail } from "@/components/RecordDetail";
import type { ViewRecord } from "@/types/view-record";

describe("RecordDetail origin agent", () => {
  it("shows the platform origin when a view record carries it", () => {
    const record: ViewRecord = {
      id: "evt-1",
      seq: 1,
      session_id: "sess-1",
      timestamp: "2026-05-24T13:00:00Z",
      origin_agent: "codex",
      agent_id: null,
      is_sidechain: false,
      record_type: "system_event",
      payload: {
        subtype: "system.session_start",
      },
    };

    render(<RecordDetail record={record} />);

    expect(screen.getByTestId("detail-origin-agent")).toHaveTextContent("codex");
  });
});
