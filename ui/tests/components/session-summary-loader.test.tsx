import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { SessionSummaryLoader } from "@/components/viz/SessionSummaryLoader";

const RECORDS = [
  { id: "e1", seq: 1, session_id: "s1", timestamp: "2026-06-30T10:00:00.000Z", record_type: "assistant_message", payload: { model: "claude-opus-4-8", content: [] }, origin_agent: "claude-code", agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50 },
  { id: "e2", seq: 2, session_id: "s1", timestamp: "2026-06-30T10:00:30.000Z", record_type: "tool_call", payload: { call_id: "c1", name: "Bash", typed_input: { tool: "bash", command: "x" } }, origin_agent: "claude-code", agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50 },
];

afterEach(() => vi.unstubAllGlobals());

describe("SessionSummaryLoader", () => {
  it("shows a skeleton while loading, then the summary header", async () => {
    let resolve!: (v: unknown) => void;
    vi.stubGlobal("fetch", vi.fn(() => new Promise((r) => { resolve = r; })));

    const { container } = render(<SessionSummaryLoader sessionId="s1" />);
    expect(screen.getByTestId("summary-loading")).toBeInTheDocument();

    resolve({ ok: true, json: async () => RECORDS });
    await waitFor(() => expect(screen.getByText(/opus-4-8/)).toBeInTheDocument());
    expect(container).toHaveTextContent(/1 tool/i);
  });

  it("renders nothing when the session has no records", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, json: async () => [] })));
    const { container } = render(<SessionSummaryLoader sessionId="empty" />);
    await waitFor(() => expect(screen.queryByTestId("summary-loading")).toBeNull());
    expect(container).toBeEmptyDOMElement();
  });
});
