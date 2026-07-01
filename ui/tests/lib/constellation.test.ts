import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildConstellation } from "@/lib/constellation";
import type { StorySession } from "@/lib/story-api";
import type { WireRecord } from "@/types/wire-record";
import type { RecordType } from "@/types/view-record";

function rec(record_type: RecordType, seq: number, payload: unknown): WireRecord {
  return {
    id: `e${seq}`, seq, session_id: "root", timestamp: `2026-06-30T10:00:0${seq}.000Z`,
    record_type, payload: payload as WireRecord["payload"], origin_agent: "claude-code",
    agent_id: null, is_sidechain: false, depth: 0, parent_uuid: null, truncated: false, payload_bytes: 50,
  };
}
const agentCall = (seq: number, cid: string, desc: string) =>
  rec("tool_call", seq, { call_id: cid, name: "Agent", typed_input: { tool: "agent", description: desc, subagent_type: "general-purpose", prompt: "x" } });
const agentResult = (seq: number, cid: string, agentId: string) =>
  rec("tool_result", seq, { call_id: cid, is_error: false, output: `agentId: ${agentId}` });

function sess(id: string, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, event_count: 10, status: "completed", total_output_tokens: 100, ...over };
}

describe("buildConstellation", () => {
  it("places the root at center with one node+edge per spawned subagent", () => {
    scenario(
      () => {
        const records = [
          agentCall(1, "c1", "Refine beachhead"), agentResult(2, "c1", "aaa111"),
          agentCall(3, "c2", "Size TAM"), agentResult(4, "c2", "bbb222"),
        ];
        const byId = new Map([
          ["root", sess("root", { event_count: 300 })],
          ["agent-aaa111", sess("agent-aaa111", { event_count: 50 })],
          ["agent-bbb222", sess("agent-bbb222", { event_count: 40 })],
        ]);
        return buildConstellation("root", records, byId);
      },
      (g) => g,
      (g) => {
        expect(g.nodes).toHaveLength(3);
        expect(g.edges).toHaveLength(2);
        const root = g.nodes.find((n) => n.id === "root")!;
        expect(root.kind).toBe("root");
        expect(root.x).toBeCloseTo(0.5);
        expect(root.y).toBeCloseTo(0.5);
        // subagents carry stats from the sessions list
        const child = g.nodes.find((n) => n.id === "agent-aaa111")!;
        expect(child.kind).toBe("subagent");
        expect(child.events).toBe(50);
        expect(child.label).toBe("Refine beachhead");
        // edges connect root → each child
        expect(g.edges.every((e) => e.from === "root")).toBe(true);
      },
    );
  });

  it("keeps an unlinked subagent as a placeholder node (no stats)", () => {
    scenario(
      () => buildConstellation("root", [agentCall(1, "c9", "in flight")], new Map([["root", sess("root")]])),
      (g) => g,
      (g) => {
        expect(g.nodes).toHaveLength(2);
        const child = g.nodes.find((n) => n.kind === "subagent")!;
        expect(child.events).toBe(0);
        expect(child.linked).toBe(false);
      },
    );
  });

  it("returns just the root when no subagents were spawned", () => {
    scenario(
      () => buildConstellation("root", [rec("tool_call", 1, { call_id: "b", name: "Bash", typed_input: { tool: "bash", command: "ls" } })], new Map([["root", sess("root")]])),
      (g) => g,
      (g) => {
        expect(g.nodes).toHaveLength(1);
        expect(g.edges).toHaveLength(0);
      },
    );
  });

  it("distributes subagents around the root (distinct angles)", () => {
    scenario(
      () => {
        const records = [
          agentCall(1, "c1", "a"), agentResult(2, "c1", "111"),
          agentCall(3, "c2", "b"), agentResult(4, "c2", "222"),
          agentCall(5, "c3", "c"), agentResult(6, "c3", "333"),
        ];
        const byId = new Map([["root", sess("root")]]);
        return buildConstellation("root", records, byId);
      },
      (g) => g.nodes.filter((n) => n.kind === "subagent").map((n) => `${n.x.toFixed(3)},${n.y.toFixed(3)}`),
      (positions) => {
        // three distinct positions, none at the exact center
        expect(new Set(positions).size).toBe(3);
        expect(positions).not.toContain("0.500,0.500");
      },
    );
  });
});
