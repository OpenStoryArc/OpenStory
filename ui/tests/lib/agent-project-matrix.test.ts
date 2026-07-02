import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildAgentProjectMatrix } from "@/lib/agent-project-matrix";

const s = (origin_agent: string, project_name: string, event_count = 10) => ({ origin_agent, project_name, event_count });

describe("buildAgentProjectMatrix", () => {
  it("aggregates sessions/events per (agent, project) cell", () => {
    scenario(
      () => buildAgentProjectMatrix([s("claude-code", "P1", 5), s("claude-code", "P1", 15), s("pi-mono", "P2", 20)]),
      (m) => m,
      (m) => {
        expect(m.agents).toContain("claude-code");
        const cc_p1 = m.cells.find((c) => c.agent === "claude-code" && c.project === "P1");
        expect(cc_p1).toEqual({ agent: "claude-code", project: "P1", sessions: 2, events: 20 });
        expect(m.maxEvents).toBe(20);
      },
    );
  });

  it("folds projects beyond top-N into an 'other' column", () => {
    scenario(
      // P1,P2,P3 heavy; P4 tiny → with topProjects=3, P4 folds into other
      () => buildAgentProjectMatrix([
        s("a", "P1", 100), s("a", "P2", 90), s("a", "P3", 80), s("a", "P4", 1), s("a", "P5", 1),
      ], { topProjects: 3 }),
      (m) => m,
      (m) => {
        expect(m.projects).toEqual(["P1", "P2", "P3", "other"]);
        const other = m.cells.find((c) => c.project === "other");
        expect(other?.sessions).toBe(2); // P4 + P5
        expect(other?.events).toBe(2);
      },
    );
  });

  it("ranks agents by session count", () => {
    scenario(
      () => buildAgentProjectMatrix([s("busy", "P", 1), s("busy", "P", 1), s("busy", "P", 1), s("quiet", "P", 1)]),
      (m) => m.agents,
      (agents) => expect(agents[0]).toBe("busy"),
    );
  });
});
