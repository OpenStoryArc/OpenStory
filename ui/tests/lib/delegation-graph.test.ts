import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { extractChildAgentIds, buildDelegationGraph, type DelSession, type DelRecord } from "@/lib/delegation-graph";

const toolResult = (text: string): DelRecord => ({ record_type: "tool_result", payload: { output: text } });

describe("extractChildAgentIds", () => {
  it("pulls agentId hexes from tool_result payloads only", () => {
    scenario(
      () => extractChildAgentIds([
        toolResult("done successfully.\nagentId: af58edebc322d73ba (internal ID - do not mention)"),
        { record_type: "tool_call", payload: { input: "agentId: deadbeef99 mentioned in code" } }, // ignored (not a result)
      ]),
      (ids) => ids,
      (ids) => expect(ids).toEqual(["af58edebc322d73ba"]),
    );
  });

  it("ignores short/non-hex mentions", () => {
    scenario(
      () => extractChildAgentIds([toolResult("readonly agentId: string; and agentId: abc (too short)")]),
      (ids) => ids,
      (ids) => expect(ids).toEqual([]),
    );
  });
});

describe("buildDelegationGraph", () => {
  const sessions: DelSession[] = [
    { session_id: "root", label: "/loop", event_count: 100, status: "completed" },
    { session_id: "agent-af58edebc322d73ba", label: "sub A", event_count: 47, status: "completed" },
  ];

  it("links a parent to a real subagent it echoed", () => {
    scenario(
      () => buildDelegationGraph(sessions, { root: [toolResult("agentId: af58edebc322d73ba (internal ID)")] }),
      (g) => g,
      (g) => {
        expect(g.links).toEqual([{ source: "root", target: "agent-af58edebc322d73ba" }]);
        expect(g.resolvedSubs).toBe(1);
        expect(g.totalSubs).toBe(1);
        expect(g.nodes.map((n) => n.id).sort()).toEqual(["agent-af58edebc322d73ba", "root"]);
        expect(g.nodes.find((n) => n.id.startsWith("agent-"))?.isSub).toBe(true);
      },
    );
  });

  it("does not link an echoed hex with no matching session (phantom filter)", () => {
    scenario(
      () => buildDelegationGraph(sessions, { root: [toolResult("agentId: 0000ffff0000 (not a real session)")] }),
      (g) => g,
      (g) => { expect(g.links).toHaveLength(0); expect(g.resolvedSubs).toBe(0); },
    );
  });
});
