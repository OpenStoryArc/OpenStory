import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildHierarchy } from "@/lib/sessions-canvas";
import type { StorySession } from "@/lib/story-api";

function sess(id: string, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, project_name: "P", user: "max", origin_agent: "claude-code", status: "completed", event_count: 10, start_time: "2026-06-10T09:00:00.000Z", ...over };
}

const SESSIONS = [
  sess("a1", { user: "max", project_name: "work" }),
  sess("a2", { user: "max", project_name: "work" }),
  sess("a3", { user: "max", project_name: "docs" }),
  sess("b1", { user: "katie", project_name: "web" }),
];

describe("buildHierarchy", () => {
  it("shows only top-level groups when nothing is expanded (progressive disclosure)", () => {
    scenario(
      () => buildHierarchy(SESSIONS, "user", new Set()),
      (h) => h,
      (h) => {
        const kinds = h.nodes.map((n) => n.kind);
        expect(new Set(kinds)).toEqual(new Set(["group"]));
        expect(h.nodes.map((n) => n.label).sort()).toEqual(["katie", "max"]);
        expect(h.nodes.find((n) => n.label === "max")!.count).toBe(3);
        expect(h.nodes.find((n) => n.label === "max")!.collapsed).toBe(true);
        expect(h.edges).toHaveLength(0);
      },
    );
  });

  it("expands a group into its projects, with parent→child edges", () => {
    scenario(
      () => buildHierarchy(SESSIONS, "user", new Set(["g:max"])),
      (h) => h,
      (h) => {
        const projects = h.nodes.filter((n) => n.kind === "project");
        expect(projects.map((p) => p.label).sort()).toEqual(["docs", "work"]);
        // still no session nodes until a project is expanded
        expect(h.nodes.some((n) => n.kind === "session")).toBe(false);
        expect(h.edges.every((e) => e.from === "g:max")).toBe(true);
      },
    );
  });

  it("drills three levels deep: group → project → sessions", () => {
    scenario(
      () => buildHierarchy(SESSIONS, "user", new Set(["g:max", "p:max:work"])),
      (h) => h,
      (h) => {
        const sessions = h.nodes.filter((n) => n.kind === "session");
        expect(sessions.map((s) => s.sessionId).sort()).toEqual(["a1", "a2"]);
        expect(sessions[0]!.level).toBe(2);
        expect(h.edges).toContainEqual({ from: "p:max:work", to: "s:a1" });
      },
    );
  });

  it("grouping by project collapses to a 2-level tree (project → session)", () => {
    scenario(
      () => buildHierarchy(SESSIONS, "project", new Set(["p:work"])),
      (h) => h,
      (h) => {
        expect(h.nodes.filter((n) => n.kind === "group")).toHaveLength(0);
        const sess = h.nodes.filter((n) => n.kind === "session");
        expect(sess.map((s) => s.sessionId).sort()).toEqual(["a1", "a2"]);
      },
    );
  });

  it("orders day-groups with the latest first", () => {
    scenario(
      () => buildHierarchy(
        [
          sess("x", { start_time: "2026-06-10T09:00:00.000Z" }),
          sess("y", { start_time: "2026-06-28T09:00:00.000Z" }),
          sess("z", { start_time: "2026-06-19T09:00:00.000Z" }),
        ],
        "day",
        new Set(),
      ),
      (h) => h.nodes.map((n) => n.label),
      (labels) => {
        // latest date first
        expect(labels[0]! > labels[1]!).toBe(true);
        expect(labels[0]).toBe(labels.slice().sort().reverse()[0]);
      },
    );
  });
});
