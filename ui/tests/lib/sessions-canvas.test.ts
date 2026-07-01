import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildCanvas } from "@/lib/sessions-canvas";
import type { StorySession } from "@/lib/story-api";

function sess(id: string, project: string, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, project_name: project, event_count: 10, status: "completed", ...over };
}

const SESSIONS = [
  sess("a1", "work"), sess("a2", "work"), sess("a3", "work"),
  sess("b1", "OpenStory"), sess("b2", "OpenStory"),
  sess("c1", "solo"),
];

describe("buildCanvas", () => {
  it("clusters by project; collapsed projects are super-nodes with no session nodes", () => {
    scenario(
      () => buildCanvas(SESSIONS, new Set()),
      (m) => m,
      (m) => {
        expect(m.clusters).toHaveLength(3); // work, OpenStory, solo
        expect(m.nodes).toHaveLength(0);
        const work = m.clusters.find((c) => c.project === "work")!;
        expect(work.count).toBe(3);
        expect(work.collapsed).toBe(true);
      },
    );
  });

  it("orders clusters by session count (biggest first) with a deterministic layout", () => {
    scenario(
      () => buildCanvas(SESSIONS, new Set()),
      (m) => m.clusters.map((c) => c.project),
      (order) => expect(order).toEqual(["work", "OpenStory", "solo"]),
    );
  });

  it("is deterministic — same input yields identical positions", () => {
    scenario(
      () => ({ a: buildCanvas(SESSIONS, new Set()), b: buildCanvas(SESSIONS, new Set()) }),
      (r) => r,
      (r) => expect(r.a.clusters.map((c) => [c.x, c.y])).toEqual(r.b.clusters.map((c) => [c.x, c.y])),
    );
  });

  it("blooms an expanded project into one node per session", () => {
    scenario(
      () => buildCanvas(SESSIONS, new Set(["work"])),
      (m) => m,
      (m) => {
        const workNodes = m.nodes.filter((n) => n.project === "work");
        expect(workNodes).toHaveLength(3);
        expect(workNodes.map((n) => n.id).sort()).toEqual(["a1", "a2", "a3"]);
        // an expanded project is no longer a collapsed super-node
        expect(m.clusters.find((c) => c.project === "work")!.collapsed).toBe(false);
        // other projects stay collapsed with no nodes
        expect(m.nodes.some((n) => n.project === "OpenStory")).toBe(false);
      },
    );
  });

  it("carries session stats onto the bloomed nodes and computes bounds", () => {
    scenario(
      () => buildCanvas([sess("x1", "p", { event_count: 42, status: "ongoing" })], new Set(["p"])),
      (m) => m,
      (m) => {
        const n = m.nodes[0]!;
        expect(n.events).toBe(42);
        expect(n.status).toBe("ongoing");
        expect(m.bounds.maxX).toBeGreaterThanOrEqual(m.bounds.minX);
      },
    );
  });
});
