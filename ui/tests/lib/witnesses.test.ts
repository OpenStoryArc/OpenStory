import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { runWitness, pearson } from "@/lib/witnesses";
import type { StorySession } from "@/lib/story-api";

function sess(id: string, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, project_name: "P", origin_agent: "claude-code", status: "completed", event_count: 10, ...over };
}

describe("runWitness", () => {
  it("grounds delegation-graph when there are ≥10 subagent sessions", () => {
    scenario(
      () => {
        const subs = Array.from({ length: 12 }, (_, i) => sess(`agent-${i}`));
        return runWitness("delegation-graph", [sess("root"), ...subs]);
      },
      (r) => r,
      (r) => { expect(r?.grounded).toBe(true); expect(r?.value).toBe(12); },
    );
  });

  it("refutes agent-project-matrix with a single agent/project", () => {
    scenario(
      () => runWitness("agent-project-matrix", [sess("a"), sess("b"), sess("c")]),
      (r) => r,
      (r) => expect(r?.grounded).toBe(false), // 1 agent × 1 project
    );
  });

  it("grounds agent-project-matrix with ≥2 agents and ≥3 projects", () => {
    scenario(
      () => runWitness("agent-project-matrix", [
        sess("a", { origin_agent: "claude-code", project_name: "P1" }),
        sess("b", { origin_agent: "pi-mono", project_name: "P2" }),
        sess("c", { origin_agent: "pi-mono", project_name: "P3" }),
      ]),
      (r) => r,
      (r) => expect(r?.grounded).toBe(true),
    );
  });

  it("returns null for a candidate with no runner (needs records)", () => {
    scenario(
      () => runWitness("tool-adjacency-heatmap", [sess("a")]),
      (r) => r,
      (r) => expect(r).toBeNull(),
    );
  });
});

describe("pearson", () => {
  it("is ~1 for a perfectly linear relationship", () => {
    expect(pearson([1, 2, 3, 4], [2, 4, 6, 8])).toBeCloseTo(1, 5);
  });
  it("returns 1 (conservative) with fewer than 3 points", () => {
    expect(pearson([1, 2], [5, 9])).toBe(1);
  });
});
