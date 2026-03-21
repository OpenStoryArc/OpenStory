import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  extractFilePath,
  extractPlanTitle,
  splitIntoTurns,
  buildEventGraph,
  applyFacets,
  fileFacets,
  toolFacets,
  planFacets,
} from "@/lib/event-graph";
import type { WireRecord } from "@/types/wire-record";

function makeRecord(id: string, overrides: Partial<WireRecord> = {}): WireRecord {
  return {
    id,
    seq: 1,
    session_id: "s1",
    timestamp: "2026-01-01T00:00:00Z",
    record_type: "assistant_message",
    payload: { text: "test" },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 100,
    ...overrides,
  } as WireRecord;
}

function toolCall(id: string, name: string, input: Record<string, unknown> = {}, typed?: Record<string, unknown>): WireRecord {
  const payload: Record<string, unknown> = { name, raw_input: input };
  if (typed) payload.typed_input = typed;
  return makeRecord(id, { record_type: "tool_call", payload }) as WireRecord;
}

function userMsg(id: string, text: string, ts: string = "2026-01-01T00:00:00Z"): WireRecord {
  return makeRecord(id, { record_type: "user_message", payload: { text }, timestamp: ts }) as WireRecord;
}

function assistantMsg(id: string, text: string): WireRecord {
  return makeRecord(id, { record_type: "assistant_message", payload: { text } }) as WireRecord;
}

function toolResult(id: string, output: string = "ok", isError: boolean = false): WireRecord {
  return makeRecord(id, { record_type: "tool_result", payload: { output, is_error: isError } }) as WireRecord;
}

function errorRecord(id: string, text: string = "boom"): WireRecord {
  return makeRecord(id, { record_type: "error", payload: { text } }) as WireRecord;
}

// ── extractFilePath — boundary table ──────────────────────

const FILE_PATH_TABLE: [string, WireRecord, string | null][] = [
  [
    "Read with typed_input",
    toolCall("1", "Read", {}, { tool: "read", file_path: "/a.ts" }),
    "/a.ts",
  ],
  [
    "Edit with typed_input",
    toolCall("2", "Edit", {}, { tool: "edit", file_path: "/b.rs" }),
    "/b.rs",
  ],
  [
    "Write with typed_input",
    toolCall("3", "Write", {}, { tool: "write", file_path: "/c.md" }),
    "/c.md",
  ],
  [
    "Grep with raw_input path",
    toolCall("4", "Grep", { path: "src/", pattern: "foo" }),
    "src/",
  ],
  [
    "Glob with raw_input path",
    toolCall("5", "Glob", { path: "ui/", pattern: "*.ts" }),
    "ui/",
  ],
  [
    "Bash (no file)",
    toolCall("6", "Bash", { command: "cargo test" }),
    null,
  ],
  [
    "Agent (no file)",
    toolCall("7", "Agent", { prompt: "research" }),
    null,
  ],
  [
    "Tool result (not a tool_call)",
    toolResult("8"),
    null,
  ],
  [
    "User message (not a tool_call)",
    userMsg("9", "hello"),
    null,
  ],
  [
    "raw_input file_path fallback",
    toolCall("10", "Read", { file_path: "/d.ts" }),
    "/d.ts",
  ],
  [
    "No input at all",
    toolCall("11", "Unknown"),
    null,
  ],
];

describe("extractFilePath — boundary table", () => {
  it.each(FILE_PATH_TABLE)(
    "%s",
    (_desc, record, expected) => {
      scenario(
        () => record,
        (r) => extractFilePath(r),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── splitIntoTurns — boundary table ──────────────────────

describe("splitIntoTurns", () => {
  it("empty records", () => {
    scenario(
      () => [] as WireRecord[],
      (r) => splitIntoTurns(r),
      (turns) => expect(turns).toHaveLength(0),
    );
  });

  it("single prompt only", () => {
    scenario(
      () => [userMsg("1", "hello")],
      (r) => splitIntoTurns(r),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.promptText).toBe("hello");
        expect(turns[0]!.eventIds).toHaveLength(1);
      },
    );
  });

  it("prompt + tools + response", () => {
    const records = [
      userMsg("1", "Fix bug"),
      toolCall("2", "Read", {}, { tool: "read", file_path: "a.ts" }),
      toolResult("3"),
      toolCall("4", "Edit", {}, { tool: "edit", file_path: "a.ts" }),
      toolResult("5"),
      assistantMsg("6", "Fixed!"),
    ];
    scenario(
      () => records,
      (r) => splitIntoTurns(r),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.eventIds).toHaveLength(6);
        expect(turns[0]!.toolCounts).toEqual({ Read: 1, Edit: 1 });
        expect(turns[0]!.files).toEqual(["a.ts"]);
        expect(turns[0]!.responseText).toBe("Fixed!");
      },
    );
  });

  it("two turns", () => {
    const records = [
      userMsg("1", "First", "2026-01-01T00:00:00Z"),
      assistantMsg("2", "Answer 1"),
      userMsg("3", "Second", "2026-01-01T00:01:00Z"),
      toolCall("4", "Bash", { command: "test" }),
      assistantMsg("5", "Answer 2"),
    ];
    scenario(
      () => records,
      (r) => splitIntoTurns(r),
      (turns) => {
        expect(turns).toHaveLength(2);
        expect(turns[0]!.promptText).toBe("First");
        expect(turns[0]!.eventIds).toHaveLength(2);
        expect(turns[1]!.promptText).toBe("Second");
        expect(turns[1]!.eventIds).toHaveLength(3);
        expect(turns[1]!.toolCounts).toEqual({ Bash: 1 });
      },
    );
  });

  it("events before first prompt form turn 0", () => {
    const records = [
      toolCall("0", "Read", { file_path: "x.ts" }),
      toolResult("0r"),
      userMsg("1", "Go"),
      assistantMsg("2", "Done"),
    ];
    scenario(
      () => records,
      (r) => splitIntoTurns(r),
      (turns) => {
        expect(turns).toHaveLength(2);
        expect(turns[0]!.promptText).toBeNull();
        expect(turns[0]!.eventIds).toHaveLength(2);
        expect(turns[1]!.promptText).toBe("Go");
      },
    );
  });

  it("turn with errors has hasError=true", () => {
    const records = [
      userMsg("1", "Try"),
      errorRecord("2", "boom"),
      assistantMsg("3", "Failed"),
    ];
    scenario(
      () => records,
      (r) => splitIntoTurns(r),
      (turns) => {
        expect(turns[0]!.hasError).toBe(true);
      },
    );
  });

  it("tool_result with is_error triggers hasError", () => {
    const records = [
      userMsg("1", "Try"),
      toolCall("2", "Bash", { command: "fail" }),
      toolResult("3", "error output", true),
    ];
    scenario(
      () => records,
      (r) => splitIntoTurns(r),
      (turns) => {
        expect(turns[0]!.hasError).toBe(true);
      },
    );
  });

  it("turn with no tools has empty toolCounts", () => {
    const records = [userMsg("1", "Hello"), assistantMsg("2", "Hi")];
    scenario(
      () => records,
      (r) => splitIntoTurns(r),
      (turns) => {
        expect(turns[0]!.toolCounts).toEqual({});
        expect(turns[0]!.files).toEqual([]);
      },
    );
  });
});

// ── buildEventGraph — integration specs ──────────────────────

describe("buildEventGraph", () => {
  const records = [
    userMsg("1", "Fix bug"),
    toolCall("2", "Read", {}, { tool: "read", file_path: "a.ts" }),
    toolResult("3"),
    toolCall("4", "Read", {}, { tool: "read", file_path: "b.rs" }),
    toolResult("5"),
    toolCall("6", "Edit", {}, { tool: "edit", file_path: "a.ts" }),
    toolResult("7"),
    toolCall("8", "Bash", { command: "cargo test" }),
    toolResult("9"),
    assistantMsg("10", "Done"),
    userMsg("11", "Add tests"),
    toolCall("12", "Bash", { command: "npm test" }),
    toolResult("13", "failed", true),
    errorRecord("14", "test failed"),
    assistantMsg("15", "Fixing..."),
  ];

  it("file index groups by path", () => {
    scenario(
      () => records,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.fileIndex.get("a.ts")).toEqual(["2", "6"]);
        expect(graph.fileIndex.get("b.rs")).toEqual(["4"]);
      },
    );
  });

  it("tool index groups by name", () => {
    scenario(
      () => records,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.toolIndex.get("Read")).toEqual(["2", "4"]);
        expect(graph.toolIndex.get("Bash")).toEqual(["8", "12"]);
        expect(graph.toolIndex.get("Edit")).toEqual(["6"]);
      },
    );
  });

  it("turns have correct tool counts", () => {
    scenario(
      () => records,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.turns[0]!.toolCounts).toEqual({ Read: 2, Edit: 1, Bash: 1 });
        expect(graph.turns[1]!.toolCounts).toEqual({ Bash: 1 });
      },
    );
  });

  it("turns have correct files", () => {
    scenario(
      () => records,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.turns[0]!.files).toEqual(["a.ts", "b.rs"]);
      },
    );
  });

  it("error IDs collected", () => {
    scenario(
      () => records,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.errorIds).toContain("13"); // tool_result is_error
        expect(graph.errorIds).toContain("14"); // error record
      },
    );
  });

  it("agent index groups by agent_id", () => {
    const withAgent = [
      ...records.slice(0, 5),
      makeRecord("a1", { record_type: "tool_call", agent_id: "sub1", payload: { name: "Read" } }),
      makeRecord("a2", { record_type: "tool_call", agent_id: "sub1", payload: { name: "Edit" } }),
    ];
    scenario(
      () => withAgent,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.agentIndex.get("sub1")).toEqual(["a1", "a2"]);
      },
    );
  });
});

// ── applyFacets — boundary table ──────────────────────

describe("applyFacets", () => {
  const records = [
    userMsg("1", "Fix bug"),
    toolCall("2", "Read", {}, { tool: "read", file_path: "a.ts" }),
    toolResult("3"),
    toolCall("4", "Bash", { command: "test" }),
    toolResult("5"),
    assistantMsg("6", "Done"),
    userMsg("7", "More"),
    toolCall("8", "Edit", {}, { tool: "edit", file_path: "a.ts" }),
    toolResult("9"),
  ];
  const graph = buildEventGraph(records);

  it("no facets returns all events", () => {
    scenario(
      () => ({ graph, records, facets: {} }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => expect(ids).toHaveLength(9),
    );
  });

  it("turn facet", () => {
    scenario(
      () => ({ graph, records, facets: { turn: 0 } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => {
        expect(ids).toHaveLength(6);
        expect(ids).toContain("1");
        expect(ids).not.toContain("7");
      },
    );
  });

  it("file facet", () => {
    scenario(
      () => ({ graph, records, facets: { file: "a.ts" } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => {
        expect(ids).toHaveLength(2);
        expect(ids).toEqual(["2", "8"]);
      },
    );
  });

  it("tool facet", () => {
    scenario(
      () => ({ graph, records, facets: { tool: "Bash" } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => {
        expect(ids).toHaveLength(1);
        expect(ids[0]).toBe("4");
      },
    );
  });

  it("turn + file intersection", () => {
    scenario(
      () => ({ graph, records, facets: { turn: 0, file: "a.ts" } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => {
        expect(ids).toHaveLength(1);
        expect(ids[0]).toBe("2"); // Read a.ts in turn 0
      },
    );
  });

  it("no match returns empty", () => {
    scenario(
      () => ({ graph, records, facets: { file: "nonexistent.ts" } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => expect(ids).toHaveLength(0),
    );
  });

  it("invalid turn returns empty", () => {
    scenario(
      () => ({ graph, records, facets: { turn: 999 } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => expect(ids).toHaveLength(0),
    );
  });
});

// ── fileFacets ──────────────────────

describe("fileFacets", () => {
  it("sorted by count descending with read/write breakdown", () => {
    const records = [
      toolCall("1", "Read", {}, { tool: "read", file_path: "a.ts" }),
      toolCall("2", "Read", {}, { tool: "read", file_path: "a.ts" }),
      toolCall("3", "Edit", {}, { tool: "edit", file_path: "a.ts" }),
      toolCall("4", "Read", {}, { tool: "read", file_path: "b.rs" }),
    ];
    const graph = buildEventGraph(records);

    scenario(
      () => ({ graph, records }),
      ({ graph, records }) => fileFacets(graph, records),
      (facets) => {
        expect(facets).toHaveLength(2);
        expect(facets[0]!.path).toBe("a.ts");
        expect(facets[0]!.count).toBe(3);
        expect(facets[0]!.reads).toBe(2);
        expect(facets[0]!.writes).toBe(1);
        expect(facets[1]!.path).toBe("b.rs");
        expect(facets[1]!.count).toBe(1);
      },
    );
  });
});

// ── toolFacets ──────────────────────

describe("toolFacets", () => {
  it("sorted by count with turn count", () => {
    const records = [
      userMsg("1", "go"),
      toolCall("2", "Read", {}, { tool: "read", file_path: "a.ts" }),
      toolCall("3", "Read", {}, { tool: "read", file_path: "b.ts" }),
      toolCall("4", "Bash", { command: "test" }),
      userMsg("5", "more"),
      toolCall("6", "Read", {}, { tool: "read", file_path: "c.ts" }),
    ];
    const graph = buildEventGraph(records);

    scenario(
      () => graph,
      (g) => toolFacets(g),
      (facets) => {
        expect(facets[0]!.name).toBe("Read");
        expect(facets[0]!.count).toBe(3);
        expect(facets[0]!.turnCount).toBe(2); // used in both turns
        expect(facets[1]!.name).toBe("Bash");
        expect(facets[1]!.count).toBe(1);
        expect(facets[1]!.turnCount).toBe(1);
      },
    );
  });
});

// ── extractPlanTitle — boundary table ──────────────────────

const PLAN_TITLE_TABLE: [string, WireRecord, string | null][] = [
  [
    "ExitPlanMode with plan content (typed_input) — extracts first heading",
    toolCall("p1", "ExitPlanMode", { plan: "# Fix auth bug\n\nSteps:\n1. Check token" }, { tool: "exit_plan_mode", plan: "# Fix auth bug\n\nSteps:\n1. Check token" }),
    "Fix auth bug",
  ],
  [
    "ExitPlanMode with plan content (raw_input only) — extracts first heading",
    toolCall("p2", "ExitPlanMode", { plan: "# Refactor database layer\n\nMove to SQLite" }),
    "Refactor database layer",
  ],
  [
    "ExitPlanMode with no heading — uses first line",
    toolCall("p3", "ExitPlanMode", { plan: "Just a plain text plan" }),
    "Just a plain text plan",
  ],
  [
    "ExitPlanMode with no plan content — returns fallback",
    toolCall("p4", "ExitPlanMode", {}),
    "Untitled plan",
  ],
  [
    "ExitPlanMode with empty plan — returns fallback",
    toolCall("p5", "ExitPlanMode", { plan: "" }),
    "Untitled plan",
  ],
  [
    "EnterPlanMode — returns plan mode label",
    toolCall("p6", "EnterPlanMode", {}),
    "[plan mode]",
  ],
  [
    "Non-plan tool (Read) — returns null",
    toolCall("p7", "Read", {}, { tool: "read", file_path: "/a.ts" }),
    null,
  ],
  [
    "Non-plan tool (Bash) — returns null",
    toolCall("p8", "Bash", { command: "cargo test" }),
    null,
  ],
  [
    "Non-tool_call record (user_message) — returns null",
    userMsg("p9", "hello"),
    null,
  ],
  [
    "Non-tool_call record (tool_result) — returns null",
    toolResult("p10"),
    null,
  ],
];

describe("extractPlanTitle — boundary table", () => {
  it.each(PLAN_TITLE_TABLE)(
    "%s",
    (_desc, record, expected) => {
      scenario(
        () => record,
        (r) => extractPlanTitle(r),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── planIndex in buildEventGraph ──────────────────────

describe("buildEventGraph — planIndex", () => {
  it("indexes ExitPlanMode events by plan title", () => {
    const records = [
      userMsg("1", "Plan things"),
      toolCall("2", "EnterPlanMode", {}),
      toolCall("3", "ExitPlanMode", { plan: "# Fix auth\n\nDetails" }),
      userMsg("4", "Another plan"),
      toolCall("5", "EnterPlanMode", {}),
      toolCall("6", "ExitPlanMode", { plan: "# Add tests\n\nMore details" }),
      toolCall("7", "ExitPlanMode", { plan: "# Fix auth\n\nUpdated plan" }),
    ];
    scenario(
      () => records,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.planIndex.size).toBe(3); // "Fix auth", "Add tests", "[plan mode]"
        expect(graph.planIndex.get("Fix auth")).toEqual(["3", "7"]);
        expect(graph.planIndex.get("Add tests")).toEqual(["6"]);
        expect(graph.planIndex.get("[plan mode]")).toEqual(["2", "5"]);
      },
    );
  });

  it("empty when no plan events", () => {
    const records = [
      userMsg("1", "Do stuff"),
      toolCall("2", "Bash", { command: "test" }),
      assistantMsg("3", "Done"),
    ];
    scenario(
      () => records,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.planIndex.size).toBe(0);
      },
    );
  });

  it("only plan events indexed (non-plan events excluded)", () => {
    const records = [
      userMsg("1", "Go"),
      toolCall("2", "Read", {}, { tool: "read", file_path: "a.ts" }),
      toolCall("3", "ExitPlanMode", { plan: "# My plan" }),
      toolCall("4", "Bash", { command: "test" }),
    ];
    scenario(
      () => records,
      (r) => buildEventGraph(r),
      (graph) => {
        expect(graph.planIndex.size).toBe(1);
        expect(graph.planIndex.get("My plan")).toEqual(["3"]);
      },
    );
  });
});

// ── planFacets ──────────────────────

describe("planFacets", () => {
  it("sorted by count descending", () => {
    const records = [
      toolCall("1", "ExitPlanMode", { plan: "# Plan A\n\nDetails" }),
      toolCall("2", "ExitPlanMode", { plan: "# Plan B\n\nDetails" }),
      toolCall("3", "ExitPlanMode", { plan: "# Plan A\n\nUpdated" }),
      toolCall("4", "ExitPlanMode", { plan: "# Plan A\n\nAgain" }),
    ];
    const graph = buildEventGraph(records);

    scenario(
      () => graph,
      (g) => planFacets(g),
      (facets) => {
        expect(facets).toHaveLength(2);
        expect(facets[0]!.title).toBe("Plan A");
        expect(facets[0]!.count).toBe(3);
        expect(facets[1]!.title).toBe("Plan B");
        expect(facets[1]!.count).toBe(1);
      },
    );
  });

  it("empty graph returns empty facets", () => {
    const graph = buildEventGraph([]);
    scenario(
      () => graph,
      (g) => planFacets(g),
      (facets) => expect(facets).toHaveLength(0),
    );
  });
});

// ── applyFacets with plan ──────────────────────

describe("applyFacets — plan facet", () => {
  const records = [
    userMsg("1", "Plan work"),
    toolCall("2", "EnterPlanMode", {}),
    toolCall("3", "ExitPlanMode", { plan: "# Fix auth\n\nSteps" }),
    toolCall("4", "Bash", { command: "test" }),
    userMsg("5", "More work"),
    toolCall("6", "ExitPlanMode", { plan: "# Add tests\n\nDetails" }),
    toolCall("7", "Read", {}, { tool: "read", file_path: "a.ts" }),
  ];
  const graph = buildEventGraph(records);

  it("plan facet filters to that plan's events", () => {
    scenario(
      () => ({ graph, records, facets: { plan: "Fix auth" } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => {
        expect(ids).toEqual(["3"]);
      },
    );
  });

  it("plan + turn intersection", () => {
    scenario(
      () => ({ graph, records, facets: { plan: "[plan mode]", turn: 0 } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => {
        expect(ids).toEqual(["2"]); // EnterPlanMode in turn 0
      },
    );
  });

  it("plan + file intersection (no overlap = empty)", () => {
    scenario(
      () => ({ graph, records, facets: { plan: "Fix auth", file: "a.ts" } }),
      ({ graph, records, facets }) => applyFacets(graph, records, facets),
      (ids) => {
        expect(ids).toHaveLength(0); // plan events and file events don't overlap
      },
    );
  });
});
