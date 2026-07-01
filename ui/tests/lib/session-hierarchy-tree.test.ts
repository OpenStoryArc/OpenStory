import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildHierarchyTree, metricValue, type TreeNode } from "@/lib/session-hierarchy-tree";
import type { StorySession } from "@/lib/story-api";

function sess(id: string, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, project_name: "P", user: "max", origin_agent: "claude-code", status: "completed", event_count: 10, total_input_tokens: 100, total_output_tokens: 50, start_time: "2026-06-10T09:00:00.000Z", ...over };
}

const leaves = (n: TreeNode): TreeNode[] => n.children ? n.children.flatMap(leaves) : [n];

describe("metricValue", () => {
  it("reads events and input+output tokens", () => {
    const s = sess("x", { event_count: 7, total_input_tokens: 300, total_output_tokens: 40 });
    expect(metricValue(s, "events")).toBe(7);
    expect(metricValue(s, "tokens")).toBe(340);
  });
});

describe("buildHierarchyTree", () => {
  it("builds root → group → project → session with session leaves valued by the metric", () => {
    scenario(
      () => buildHierarchyTree(
        [sess("a1", { user: "max", project_name: "work", event_count: 5 }), sess("b1", { user: "katie", project_name: "web", event_count: 9 })],
        "user", "events",
      ),
      (root) => root,
      (root) => {
        expect(root.kind).toBe("root");
        expect(root.children!.map((g) => g.name).sort()).toEqual(["katie", "max"]);
        const leafA = leaves(root).find((n) => n.sessionId === "a1")!;
        expect(leafA.kind).toBe("session");
        expect(leafA.value).toBe(5);
      },
    );
  });

  it("orders siblings by magnitude (biggest first)", () => {
    scenario(
      () => buildHierarchyTree(
        [sess("s", { user: "small", event_count: 1 }), sess("b", { user: "big", event_count: 500 })],
        "user", "events",
      ),
      (root) => root.children!.map((g) => g.name),
      (order) => expect(order).toEqual(["big", "small"]),
    );
  });

  it("caps children at top-N and rolls the rest into an 'other (K)' node", () => {
    scenario(
      () => {
        // 5 distinct projects under one user, topN=2 → keep 2 + other(3)
        const ss = ["p1", "p2", "p3", "p4", "p5"].map((p, i) => sess(`s${i}`, { user: "max", project_name: p, event_count: 10 - i }));
        return buildHierarchyTree(ss, "user", "events", { topN: 2 });
      },
      (root) => root.children!.find((g) => g.name === "max")!.children!,
      (projects) => {
        expect(projects).toHaveLength(3); // 2 kept + 1 other
        const other = projects.find((p) => p.kind === "other")!;
        expect(other.name).toBe("other (3)");
        expect(other.value).toBeGreaterThan(0);
      },
    );
  });

  it("grouping by project produces a 2-level tree (project → session)", () => {
    scenario(
      () => buildHierarchyTree([sess("a1", { project_name: "work" }), sess("a2", { project_name: "work" })], "project", "tokens"),
      (root) => root,
      (root) => {
        expect(root.children!.every((c) => c.kind === "project")).toBe(true);
        expect(leaves(root).map((l) => l.sessionId).sort()).toEqual(["a1", "a2"]);
      },
    );
  });
});
