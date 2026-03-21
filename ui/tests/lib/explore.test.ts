import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  groupSessionsByDay,
  filterSessionsByQuery,
  filterSessionsByStatus,
  filterSessionsByProject,
  extractProjects,
  computeStatusCounts,
  dayLabel,
  isAgentSession,
  buildSessionHierarchy,
  type ParentSession,
} from "@/lib/explore";
import type { SessionSummary } from "@/types/session";

function makeSession(overrides: Partial<SessionSummary> & { session_id: string; start_time: string }): SessionSummary {
  return {
    status: "completed",
    event_count: 10,
    ...overrides,
  };
}

// ── dayLabel — boundary table ──────────────────────

const NOW = new Date("2025-01-16T12:00:00Z");

const DAY_LABEL_TABLE: [string, string, string][] = [
  ["today", "2025-01-16T08:00:00Z", "Today"],
  ["yesterday", "2025-01-15T20:00:00Z", "Yesterday"],
  ["3 days ago (weekday)", "2025-01-13T12:00:00Z", "Monday"],
  ["6 days ago (weekday)", "2025-01-10T12:00:00Z", "Friday"],
  ["8 days ago (date)", "2025-01-08T12:00:00Z", "Jan 8, 2025"],
  ["last year", "2024-12-25T12:00:00Z", "Dec 25, 2024"],
];

describe("dayLabel — boundary table", () => {
  it.each(DAY_LABEL_TABLE)(
    "%s",
    (_desc, iso, expected) => {
      scenario(
        () => iso,
        (ts) => dayLabel(ts, NOW),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── groupSessionsByDay ──────────────────────

describe("groupSessionsByDay", () => {
  it("returns empty for no sessions", () => {
    scenario(
      () => [] as SessionSummary[],
      (sessions) => groupSessionsByDay(sessions, NOW),
      (groups) => expect(groups).toEqual([]),
    );
  });

  it("groups sessions by day, sorted newest first", () => {
    const sessions = [
      makeSession({ session_id: "a", start_time: "2025-01-16T10:00:00Z" }),
      makeSession({ session_id: "b", start_time: "2025-01-16T08:00:00Z" }),
      makeSession({ session_id: "c", start_time: "2025-01-15T12:00:00Z" }),
    ];

    scenario(
      () => sessions,
      (s) => groupSessionsByDay(s, NOW),
      (groups) => {
        expect(groups).toHaveLength(2);
        expect(groups[0]!.label).toBe("Today");
        expect(groups[0]!.sessions).toHaveLength(2);
        expect(groups[0]!.sessions[0]!.session_id).toBe("a"); // newer first
        expect(groups[1]!.label).toBe("Yesterday");
        expect(groups[1]!.sessions).toHaveLength(1);
      },
    );
  });
});

// ── filterSessionsByQuery ──────────────────────

describe("filterSessionsByQuery", () => {
  const sessions = [
    makeSession({ session_id: "abc-123", start_time: "2025-01-16T10:00:00Z", first_prompt: "Fix the auth bug", project_name: "MyProject" }),
    makeSession({ session_id: "def-456", start_time: "2025-01-16T08:00:00Z", first_prompt: "Add unit tests" }),
    makeSession({ session_id: "ghi-789", start_time: "2025-01-15T12:00:00Z", first_prompt: "Refactor parser", project_name: "Parser" }),
  ];

  it("empty query returns all", () => {
    scenario(
      () => ({ sessions, query: "" }),
      ({ sessions, query }) => filterSessionsByQuery(sessions, query),
      (result) => expect(result).toHaveLength(3),
    );
  });

  it("matches session_id", () => {
    scenario(
      () => ({ sessions, query: "abc" }),
      ({ sessions, query }) => filterSessionsByQuery(sessions, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.session_id).toBe("abc-123");
      },
    );
  });

  it("matches first_prompt", () => {
    scenario(
      () => ({ sessions, query: "auth" }),
      ({ sessions, query }) => filterSessionsByQuery(sessions, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.session_id).toBe("abc-123");
      },
    );
  });

  it("matches project_name", () => {
    scenario(
      () => ({ sessions, query: "parser" }),
      ({ sessions, query }) => filterSessionsByQuery(sessions, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.session_id).toBe("ghi-789");
      },
    );
  });

  it("case insensitive", () => {
    scenario(
      () => ({ sessions, query: "UNIT" }),
      ({ sessions, query }) => filterSessionsByQuery(sessions, query),
      (result) => {
        expect(result).toHaveLength(1);
        expect(result[0]!.session_id).toBe("def-456");
      },
    );
  });

  it("no match returns empty", () => {
    scenario(
      () => ({ sessions, query: "zzz" }),
      ({ sessions, query }) => filterSessionsByQuery(sessions, query),
      (result) => expect(result).toHaveLength(0),
    );
  });
});

// ── filterSessionsByStatus — boundary table ──────────────────────

const STATUS_SESSIONS = [
  makeSession({ session_id: "a", start_time: "2025-01-16T10:00:00Z", status: "ongoing" }),
  makeSession({ session_id: "b", start_time: "2025-01-16T09:00:00Z", status: "completed" }),
  makeSession({ session_id: "c", start_time: "2025-01-16T08:00:00Z", status: "errored" }),
  makeSession({ session_id: "d", start_time: "2025-01-16T07:00:00Z", status: "completed" }),
  makeSession({ session_id: "e", start_time: "2025-01-16T06:00:00Z", status: "stale" }),
];

const STATUS_TABLE: [string, "all" | "ongoing" | "completed" | "errored" | "stale", number][] = [
  ["all returns everything", "all", 5],
  ["ongoing", "ongoing", 1],
  ["completed", "completed", 2],
  ["errored", "errored", 1],
  ["stale", "stale", 1],
];

describe("filterSessionsByStatus — boundary table", () => {
  it.each(STATUS_TABLE)(
    "%s",
    (_desc, status, expectedCount) => {
      scenario(
        () => ({ sessions: STATUS_SESSIONS, status }),
        ({ sessions, status }) => filterSessionsByStatus(sessions, status),
        (result) => expect(result).toHaveLength(expectedCount),
      );
    },
  );
});

// ── filterSessionsByProject ──────────────────────

describe("filterSessionsByProject", () => {
  const sessions = [
    makeSession({ session_id: "a", start_time: "2025-01-16T10:00:00Z", project_id: "proj-1", project_name: "Alpha" }),
    makeSession({ session_id: "b", start_time: "2025-01-16T09:00:00Z", project_id: "proj-1", project_name: "Alpha" }),
    makeSession({ session_id: "c", start_time: "2025-01-16T08:00:00Z", project_id: "proj-2", project_name: "Beta" }),
  ];

  it("empty project returns all", () => {
    scenario(
      () => ({ sessions, project: "" }),
      ({ sessions, project }) => filterSessionsByProject(sessions, project),
      (result) => expect(result).toHaveLength(3),
    );
  });

  it("filters by project_id", () => {
    scenario(
      () => ({ sessions, project: "proj-1" }),
      ({ sessions, project }) => filterSessionsByProject(sessions, project),
      (result) => {
        expect(result).toHaveLength(2);
        expect(result.every((s) => s.project_id === "proj-1")).toBe(true);
      },
    );
  });
});

// ── extractProjects ──────────────────────

describe("extractProjects", () => {
  it("returns empty for no sessions", () => {
    scenario(
      () => [] as SessionSummary[],
      (sessions) => extractProjects(sessions),
      (projects) => expect(projects).toEqual([]),
    );
  });

  it("extracts unique projects sorted by count", () => {
    const sessions = [
      makeSession({ session_id: "a", start_time: "2025-01-16T10:00:00Z", project_id: "p1", project_name: "Alpha" }),
      makeSession({ session_id: "b", start_time: "2025-01-16T09:00:00Z", project_id: "p1", project_name: "Alpha" }),
      makeSession({ session_id: "c", start_time: "2025-01-16T08:00:00Z", project_id: "p2", project_name: "Beta" }),
    ];
    scenario(
      () => sessions,
      (s) => extractProjects(s),
      (projects) => {
        expect(projects).toHaveLength(2);
        expect(projects[0]!.id).toBe("p1");
        expect(projects[0]!.count).toBe(2);
        expect(projects[1]!.id).toBe("p2");
        expect(projects[1]!.count).toBe(1);
      },
    );
  });

  it("skips sessions with no project_id", () => {
    const sessions = [
      makeSession({ session_id: "a", start_time: "2025-01-16T10:00:00Z" }),
      makeSession({ session_id: "b", start_time: "2025-01-16T09:00:00Z", project_id: "p1", project_name: "X" }),
    ];
    scenario(
      () => sessions,
      (s) => extractProjects(s),
      (projects) => {
        expect(projects).toHaveLength(1);
        expect(projects[0]!.id).toBe("p1");
      },
    );
  });
});

// ── computeStatusCounts ──────────────────────

describe("computeStatusCounts", () => {
  it("counts all statuses", () => {
    scenario(
      () => STATUS_SESSIONS,
      (sessions) => computeStatusCounts(sessions),
      (counts) => {
        expect(counts.all).toBe(5);
        expect(counts.ongoing).toBe(1);
        expect(counts.completed).toBe(2);
        expect(counts.errored).toBe(1);
        expect(counts.stale).toBe(1);
      },
    );
  });

  it("zero counts for empty", () => {
    scenario(
      () => [] as SessionSummary[],
      (sessions) => computeStatusCounts(sessions),
      (counts) => {
        expect(counts.all).toBe(0);
        expect(counts.ongoing).toBe(0);
      },
    );
  });
});

// ── isAgentSession — boundary table ──────────────────────

const AGENT_TABLE: [string, string, boolean][] = [
  ["agent session", "agent-abc123", true],
  ["main session (uuid)", "3f9e4e34-cca3-4b21", false],
  ["main session (short)", "abc-123", false],
  ["agent- prefix exact", "agent-", true],
  ["agent without dash", "agentabc", false],
];

describe("isAgentSession — boundary table", () => {
  it.each(AGENT_TABLE)(
    "%s",
    (_desc, sessionId, expected) => {
      scenario(
        () => sessionId,
        (id) => isAgentSession(id),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── buildSessionHierarchy ──────────────────────

describe("buildSessionHierarchy", () => {
  it("returns empty for no sessions", () => {
    scenario(
      () => [] as SessionSummary[],
      (sessions) => buildSessionHierarchy(sessions),
      (parents) => expect(parents).toEqual([]),
    );
  });

  it("main session with no agents", () => {
    const sessions = [
      makeSession({ session_id: "abc-123", start_time: "2025-01-16T10:00:00Z", project_id: "p1" }),
    ];
    scenario(
      () => sessions,
      (s) => buildSessionHierarchy(s),
      (parents) => {
        expect(parents).toHaveLength(1);
        expect(parents[0]!.session.session_id).toBe("abc-123");
        expect(parents[0]!.agents).toHaveLength(0);
      },
    );
  });

  it("groups agent sessions under parent by project", () => {
    const sessions = [
      makeSession({ session_id: "abc-123", start_time: "2025-01-16T10:00:00Z", project_id: "p1" }),
      makeSession({ session_id: "agent-xyz", start_time: "2025-01-16T10:01:00Z", project_id: "p1" }),
      makeSession({ session_id: "agent-qqq", start_time: "2025-01-16T10:02:00Z", project_id: "p1" }),
    ];
    scenario(
      () => sessions,
      (s) => buildSessionHierarchy(s),
      (parents) => {
        expect(parents).toHaveLength(1);
        expect(parents[0]!.session.session_id).toBe("abc-123");
        expect(parents[0]!.agents).toHaveLength(2);
        expect(parents[0]!.agents[0]!.session_id).toBe("agent-xyz");
      },
    );
  });

  it("multiple projects keep separate hierarchies", () => {
    const sessions = [
      makeSession({ session_id: "main-1", start_time: "2025-01-16T10:00:00Z", project_id: "p1" }),
      makeSession({ session_id: "agent-a1", start_time: "2025-01-16T10:01:00Z", project_id: "p1" }),
      makeSession({ session_id: "main-2", start_time: "2025-01-16T09:00:00Z", project_id: "p2" }),
      makeSession({ session_id: "agent-a2", start_time: "2025-01-16T09:01:00Z", project_id: "p2" }),
    ];
    scenario(
      () => sessions,
      (s) => buildSessionHierarchy(s),
      (parents) => {
        expect(parents).toHaveLength(2);
        expect(parents[0]!.agents).toHaveLength(1);
        expect(parents[1]!.agents).toHaveLength(1);
      },
    );
  });

  it("orphan agents (no matching parent) become their own top-level entry", () => {
    const sessions = [
      makeSession({ session_id: "agent-orphan", start_time: "2025-01-16T10:00:00Z", project_id: "unknown" }),
    ];
    scenario(
      () => sessions,
      (s) => buildSessionHierarchy(s),
      (parents) => {
        expect(parents).toHaveLength(1);
        expect(parents[0]!.session.session_id).toBe("agent-orphan");
        expect(parents[0]!.agents).toHaveLength(0);
      },
    );
  });

  it("sorted by most recent first", () => {
    const sessions = [
      makeSession({ session_id: "old", start_time: "2025-01-14T10:00:00Z", project_id: "p1" }),
      makeSession({ session_id: "new", start_time: "2025-01-16T10:00:00Z", project_id: "p2" }),
    ];
    scenario(
      () => sessions,
      (s) => buildSessionHierarchy(s),
      (parents) => {
        expect(parents[0]!.session.session_id).toBe("new");
        expect(parents[1]!.session.session_id).toBe("old");
      },
    );
  });

  it("agent count included in parent", () => {
    const sessions = [
      makeSession({ session_id: "main", start_time: "2025-01-16T10:00:00Z", project_id: "p1", event_count: 100 }),
      makeSession({ session_id: "agent-a", start_time: "2025-01-16T10:01:00Z", project_id: "p1", event_count: 50 }),
      makeSession({ session_id: "agent-b", start_time: "2025-01-16T10:02:00Z", project_id: "p1", event_count: 30 }),
    ];
    scenario(
      () => sessions,
      (s) => buildSessionHierarchy(s),
      (parents) => {
        expect(parents[0]!.totalAgentEvents).toBe(80);
      },
    );
  });
});
