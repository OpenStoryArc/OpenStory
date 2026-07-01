import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { extractSubagents, isSubagentSession } from "@/lib/subagents";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, seq: number, payload: unknown): WireRecord {
  return {
    id: `e${seq}`, seq, session_id: "s1", timestamp: `2026-06-30T10:00:0${seq}.000Z`,
    record_type, payload: payload as WireRecord["payload"], origin_agent: "claude-code",
    agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50,
  };
}

const agentResult = (call_id: string, agentId: string, is_error = false) =>
  rec("tool_result", 2, {
    call_id,
    is_error,
    output: JSON.stringify([{ type: "text", text: `Async agent launched successfully.\nagentId: ${agentId} (internal ID)` }]),
  });

describe("isSubagentSession", () => {
  it("recognizes agent- prefixed session ids", () => {
    expect(isSubagentSession("agent-ada5a6f42bd70f812")).toBe(true);
    expect(isSubagentSession("0375729d-4f5f")).toBe(false);
  });
});

describe("extractSubagents", () => {
  it("links a spawned agent to its child session via the result's agentId", () => {
    scenario(
      () => [
        rec("tool_call", 1, { call_id: "c1", name: "Agent", typed_input: { tool: "agent", description: "Map the UI codebase", subagent_type: "Explore", prompt: "explore..." } }),
        agentResult("c1", "ada5a6f42bd70f812"),
      ],
      (records) => extractSubagents(records),
      (subs) => {
        expect(subs).toHaveLength(1);
        expect(subs[0]).toMatchObject({
          description: "Map the UI codebase",
          subagentType: "Explore",
          agentId: "ada5a6f42bd70f812",
          sessionId: "agent-ada5a6f42bd70f812",
          isError: false,
        });
      },
    );
  });

  it("keeps a spawned agent even when no result/agentId is available yet", () => {
    scenario(
      () => [rec("tool_call", 1, { call_id: "c9", name: "Agent", typed_input: { tool: "agent", description: "in flight", prompt: "..." } })],
      (records) => extractSubagents(records),
      (subs) => {
        expect(subs).toHaveLength(1);
        expect(subs[0]!.agentId).toBeNull();
        expect(subs[0]!.sessionId).toBeNull();
      },
    );
  });

  it("falls back to the prompt's first line when there's no description", () => {
    scenario(
      () => [rec("tool_call", 1, { call_id: "c2", name: "Agent", typed_input: { tool: "agent", prompt: "Audit the auth module\nsecond line" } })],
      (records) => extractSubagents(records),
      (subs) => expect(subs[0]!.description).toBe("Audit the auth module"),
    );
  });

  it("ignores non-agent tool calls", () => {
    scenario(
      () => [
        rec("tool_call", 1, { call_id: "b1", name: "Bash", typed_input: { tool: "bash", command: "ls" } }),
        rec("tool_call", 2, { call_id: "r1", name: "Read", typed_input: { tool: "read", file_path: "/a" } }),
      ],
      (records) => extractSubagents(records),
      (subs) => expect(subs).toHaveLength(0),
    );
  });

  it("flags a subagent whose result errored", () => {
    scenario(
      () => [
        rec("tool_call", 1, { call_id: "c1", name: "Agent", typed_input: { tool: "agent", description: "boom", prompt: "x" } }),
        agentResult("c1", "deadbeef01", true),
      ],
      (records) => extractSubagents(records),
      (subs) => expect(subs[0]!.isError).toBe(true),
    );
  });
});
