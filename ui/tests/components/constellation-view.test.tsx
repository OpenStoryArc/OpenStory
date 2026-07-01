import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ConstellationView } from "@/components/viz/ConstellationView";

const SESSIONS = [
  { session_id: "root", label: "TAM sizing", event_count: 300, status: "completed" },
  { session_id: "agent-aaa111", event_count: 50, status: "completed" },
  { session_id: "agent-bbb222", event_count: 40, status: "completed" },
];
const RECORDS = [
  { id: "e1", seq: 1, session_id: "root", timestamp: "2026-06-30T10:00:01Z", record_type: "tool_call", payload: { call_id: "c1", name: "Agent", typed_input: { tool: "agent", description: "Refine beachhead", subagent_type: "general-purpose", prompt: "x" } }, origin_agent: "claude-code", agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50 },
  { id: "e2", seq: 2, session_id: "root", timestamp: "2026-06-30T10:00:02Z", record_type: "tool_result", payload: { call_id: "c1", is_error: false, output: "agentId: aaa111" }, origin_agent: "claude-code", agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50 },
  { id: "e3", seq: 3, session_id: "root", timestamp: "2026-06-30T10:00:03Z", record_type: "tool_call", payload: { call_id: "c2", name: "Agent", typed_input: { tool: "agent", description: "Size TAM", subagent_type: "general-purpose", prompt: "x" } }, origin_agent: "claude-code", agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50 },
  { id: "e4", seq: 4, session_id: "root", timestamp: "2026-06-30T10:00:04Z", record_type: "tool_result", payload: { call_id: "c2", is_error: false, output: "agentId: bbb222" }, origin_agent: "claude-code", agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50 },
];

function mockFetch(records: unknown[]) {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => ({
    ok: true,
    json: async () => (String(url).includes("/records") ? records : { sessions: SESSIONS }),
  })));
}
afterEach(() => vi.unstubAllGlobals());

describe("ConstellationView", () => {
  it("renders the root + a node per subagent and links them", async () => {
    mockFetch(RECORDS);
    render(<ConstellationView rootId="root" onOpen={() => {}} />);
    await waitFor(() => expect(document.querySelectorAll("[data-con-node]")).toHaveLength(3));
    expect(document.querySelector('[data-con-node="root"]')).toBeInTheDocument();
    expect(document.querySelector('[data-con-node="agent-aaa111"]')).toBeInTheDocument();
    expect(screen.getByText("Refine beachhead")).toBeInTheDocument();
  });

  it("opens a subagent's session when its node is clicked", async () => {
    mockFetch(RECORDS);
    const onOpen = vi.fn();
    render(<ConstellationView rootId="root" onOpen={onOpen} />);
    await waitFor(() => expect(document.querySelector('[data-con-node="agent-aaa111"]')).toBeInTheDocument());
    fireEvent.click(document.querySelector('[data-con-node="agent-aaa111"]')!);
    expect(onOpen).toHaveBeenCalledWith("agent-aaa111");
  });

  it("shows an empty state for a session with no subagents", async () => {
    mockFetch([{ id: "b", seq: 1, session_id: "root", timestamp: "2026-06-30T10:00:01Z", record_type: "tool_call", payload: { call_id: "b", name: "Bash", typed_input: { tool: "bash", command: "ls" } }, origin_agent: "claude-code", agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50 }]);
    render(<ConstellationView rootId="root" />);
    await waitFor(() => expect(screen.getByTestId("constellation-empty")).toBeInTheDocument());
  });
});
