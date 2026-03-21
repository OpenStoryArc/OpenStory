import { describe, it, expect } from "vitest";
import {
  groupByProject,
  projectDisplayName,
  filterSessions,
} from "@/lib/filters";
import type { SessionSummary } from "@/types/session";

function makeSession(overrides: Partial<SessionSummary> = {}): SessionSummary {
  return {
    session_id: "s1",
    status: "ongoing",
    start_time: "2025-01-01T00:00:00Z",
    event_count: 1,
    ...overrides,
  };
}

describe("groupByProject", () => {
  it("groups sessions by project_id", () => {
    const sessions = [
      makeSession({ session_id: "s1", project_id: "proj-a" }),
      makeSession({ session_id: "s2", project_id: "proj-b" }),
      makeSession({ session_id: "s3", project_id: "proj-a" }),
    ];
    const groups = groupByProject(sessions);
    expect(groups.size).toBe(2);
    expect(groups.get("proj-a")?.length).toBe(2);
    expect(groups.get("proj-b")?.length).toBe(1);
  });

  it("sessions without project_id go to 'unknown'", () => {
    const sessions = [
      makeSession({ session_id: "s1", project_id: "proj-a" }),
      makeSession({ session_id: "s2" }), // no project_id
    ];
    const groups = groupByProject(sessions);
    expect(groups.size).toBe(2);
    expect(groups.get("unknown")?.length).toBe(1);
    expect(groups.get("proj-a")?.length).toBe(1);
  });

  it("handles empty input", () => {
    const groups = groupByProject([]);
    expect(groups.size).toBe(0);
  });
});

describe("projectDisplayName", () => {
  it("prefers project_name from API when available", () => {
    const sessions = [
      makeSession({
        cwd: "/home/user/projects/open-story/ui",
        project_name: "open-story",
      }),
    ];
    expect(projectDisplayName("C--home-user-projects-open-story", sessions)).toBe("open-story");
  });

  it("falls back to last segment of project_id when no project_name", () => {
    const sessions = [makeSession({})]; // no project_name or cwd
    expect(
      projectDisplayName("-home-user-projects-open-story", sessions),
    ).toBe("story");
  });

  it("returns 'Unknown Project' for 'unknown' id", () => {
    expect(projectDisplayName("unknown", [])).toBe("Unknown Project");
  });
});

describe("filterSessions", () => {
  it("returns all sessions when filter is 'all'", () => {
    const sessions = [
      makeSession({ status: "ongoing" }),
      makeSession({ session_id: "s2", status: "completed" }),
    ];
    expect(filterSessions(sessions, "all").length).toBe(2);
  });

  it("filters by status", () => {
    const sessions = [
      makeSession({ status: "ongoing" }),
      makeSession({ session_id: "s2", status: "completed" }),
    ];
    expect(filterSessions(sessions, "completed").length).toBe(1);
  });
});
