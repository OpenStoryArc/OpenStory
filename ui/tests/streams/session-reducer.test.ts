//! Spec: Enriched sessionReducer — durable/ephemeral separation + filter deltas.
//!
//! Phase 4 of Story 036: Stateful BFF projection.
//!
//! The enriched reducer handles WireBroadcast messages that separate
//! durable records (accumulate) from ephemeral records (show transiently).
//! It maintains filter counts via deltas and a tree index for depth/parent.
//!
//! These tests are the implementation spec. Red → green → refactor.

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";

import type { WireRecord } from "@/types/wire-record";
import {
  enrichedReducer,
  EMPTY_ENRICHED_STATE,
  toPatternView,
  getFilterCounts,
  type EnrichedSessionState,
  type EnrichedAction,
} from "@/streams/sessions";
import type { ServerPatternEvent } from "@/types/websocket";

// ── Helpers ──

function makeWireRecord(overrides: Partial<WireRecord> = {}): WireRecord {
  return {
    id: "rec-1",
    seq: 1,
    session_id: "s1",
    timestamp: "2025-01-01T00:00:00Z",
    record_type: "user_message",
    payload: { content: "hello" },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 5,
    ...overrides,
  } as WireRecord;
}

function makeEnrichedMessage(overrides: Partial<EnrichedAction & { kind: "enriched" }> = {}): EnrichedAction {
  return {
    kind: "enriched" as const,
    session_id: "s1",
    records: [],
    ephemeral: [],
    filter_deltas: {},
    ...overrides,
  };
}

const EMPTY_STATE = EMPTY_ENRICHED_STATE;

// ═══════════════════════════════════════════════════════════════════
// describe("enrichedReducer — initial_state")
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — initial_state", () => {
  it("should set records from initial_state", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: {
          kind: "initial_state" as const,
          records: [makeWireRecord({ id: "r1" }), makeWireRecord({ id: "r2", seq: 2 })],
          patterns: [],
          filterCounts: { s1: { all: 2, user: 2 } },
          sessionLabels: {},
          agentLabels: {},
        },
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.records).toHaveLength(2);
        expect(result.records[0]!.id).toBe("r1");
        expect(result.records[1]!.id).toBe("r2");
      },
    );
  });

  it("should set filterCounts from initial_state (per-session)", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: {
          kind: "initial_state" as const,
          records: [makeWireRecord()],
          patterns: [],
          filterCounts: { s1: { all: 5, user: 3, tools: 2 } },
          sessionLabels: {},
          agentLabels: {},
        },
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        const s1 = result.filterCounts["s1"]!;
        expect(s1["all"]).toBe(5);
        expect(s1["user"]).toBe(3);
        expect(s1["tools"]).toBe(2);
      },
    );
  });

  it("should populate treeIndex from initial records", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: {
          kind: "initial_state" as const,
          records: [
            makeWireRecord({ id: "r1", depth: 0, parent_uuid: null }),
            makeWireRecord({ id: "r2", depth: 1, parent_uuid: "r1", seq: 2 }),
          ],
          patterns: [],
          filterCounts: { s1: { all: 2 } },
          sessionLabels: {},
          agentLabels: {},
        },
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.treeIndex.get("r1")).toEqual({ depth: 0, parent_uuid: null });
        expect(result.treeIndex.get("r2")).toEqual({ depth: 1, parent_uuid: "r1" });
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("enrichedReducer — enriched with records (durable)")
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — durable records", () => {
  it("should append durable records to state.records", () => {
    scenario(
      () => ({
        state: { ...EMPTY_STATE, records: [makeWireRecord({ id: "existing" })] },
        action: makeEnrichedMessage({
          records: [makeWireRecord({ id: "new", seq: 2 })],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.records).toHaveLength(2);
        expect(result.records[1]!.id).toBe("new");
      },
    );
  });

  it("should update treeIndex for new durable records", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: makeEnrichedMessage({
          records: [makeWireRecord({ id: "r1", depth: 3, parent_uuid: "r0" })],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.treeIndex.get("r1")).toEqual({ depth: 3, parent_uuid: "r0" });
      },
    );
  });

  it("should NOT clear currentEphemeral when durable records arrive", () => {
    scenario(
      () => ({
        state: {
          ...EMPTY_STATE,
          currentEphemeral: makeWireRecord({ id: "progress-1", record_type: "system_event" }),
        },
        action: makeEnrichedMessage({
          records: [makeWireRecord({ id: "r1" })],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.records).toHaveLength(1);
        // Ephemeral is independent — still there until replaced
        expect(result.currentEphemeral).not.toBeNull();
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("enrichedReducer — enriched with ephemeral (progress)")
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — ephemeral records", () => {
  it("should set currentEphemeral and NOT append to records", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: makeEnrichedMessage({
          ephemeral: [makeWireRecord({ id: "progress-1", record_type: "system_event" })],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.records).toHaveLength(0); // NOT accumulated
        expect(result.currentEphemeral).not.toBeNull();
        expect(result.currentEphemeral!.id).toBe("progress-1");
      },
    );
  });

  it("should replace previous ephemeral (not accumulate)", () => {
    scenario(
      () => ({
        state: {
          ...EMPTY_STATE,
          currentEphemeral: makeWireRecord({ id: "old-progress" }),
        },
        action: makeEnrichedMessage({
          ephemeral: [makeWireRecord({ id: "new-progress", record_type: "system_event" })],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.currentEphemeral!.id).toBe("new-progress");
      },
    );
  });

  it("should NOT update treeIndex for ephemeral records", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: makeEnrichedMessage({
          ephemeral: [makeWireRecord({ id: "eph-1", depth: 5, parent_uuid: "r0" })],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.treeIndex.has("eph-1")).toBe(false);
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("enrichedReducer — patterns")
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — patterns", () => {
  it("should append new patterns to state.patterns", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: makeEnrichedMessage({
          patterns: [{ type: "git.workflow", label: "Git: commit flow", events: ["e1", "e2", "e3"] }],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.patterns).toHaveLength(1);
        expect(result.patterns[0]!.type).toBe("git.workflow");
      },
    );
  });

  it("should accumulate patterns across multiple messages", () => {
    scenario(
      () => ({
        state: {
          ...EMPTY_STATE,
          patterns: [{ type: "test.cycle", label: "Test cycle", events: ["e1"] }],
        },
        action: makeEnrichedMessage({
          patterns: [{ type: "git.workflow", label: "Git flow", events: ["e2", "e3"] }],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.patterns).toHaveLength(2);
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("enrichedReducer — filter_deltas")
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — filter_deltas", () => {
  it("should increment filterCounts by delta values (per-session)", () => {
    scenario(
      () => ({
        state: { ...EMPTY_STATE, filterCounts: { s1: { all: 10, user: 5 } } },
        action: makeEnrichedMessage({
          filter_deltas: { all: 1, user: 1, narrative: 1 },
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        const s1 = result.filterCounts["s1"]!;
        expect(s1["all"]).toBe(11);
        expect(s1["user"]).toBe(6);
        expect(s1["narrative"]).toBe(1); // new key, starts from 0
      },
    );
  });

  it("should handle zero deltas gracefully", () => {
    scenario(
      () => ({
        state: { ...EMPTY_STATE, filterCounts: { s1: { all: 5 } } },
        action: makeEnrichedMessage({ filter_deltas: {} }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.filterCounts["s1"]!["all"]).toBe(5); // unchanged
      },
    );
  });

  /// Boundary table: filter_deltas accumulation after N messages (per-session)
  ///
  /// | Message # | Deltas          | Expected filterCounts.s1       |
  /// |-----------|-----------------|-------------------------------|
  /// | 1         | {all:1, user:1} | {all:1, user:1}               |
  /// | 2         | {all:1, tools:1}| {all:2, user:1, tools:1}      |
  /// | 3         | {all:1, user:1} | {all:3, user:2, tools:1}      |
  it("should accumulate deltas correctly over multiple messages", () => {
    const messages: EnrichedAction[] = [
      makeEnrichedMessage({ filter_deltas: { all: 1, user: 1 } }),
      makeEnrichedMessage({ filter_deltas: { all: 1, tools: 1 } }),
      makeEnrichedMessage({ filter_deltas: { all: 1, user: 1 } }),
    ];

    let state: EnrichedSessionState = EMPTY_STATE;
    for (const msg of messages) {
      state = enrichedReducer(state, msg);
    }

    const s1 = state.filterCounts["s1"]!;
    expect(s1["all"]).toBe(3);
    expect(s1["user"]).toBe(2);
    expect(s1["tools"]).toBe(1);
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("enrichedReducer — truncation awareness")
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — truncation", () => {
  it("should preserve truncated flag on records", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: makeEnrichedMessage({
          records: [makeWireRecord({ id: "big", truncated: true, payload_bytes: 50000 })],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.records[0]!.truncated).toBe(true);
        expect(result.records[0]!.payload_bytes).toBe(50000);
      },
    );
  });

  it("should mark non-truncated records correctly", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: makeEnrichedMessage({
          records: [makeWireRecord({ id: "small", truncated: false, payload_bytes: 100 })],
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        expect(result.records[0]!.truncated).toBe(false);
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("enrichedReducer — combined message")
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — combined records + ephemeral + patterns + deltas", () => {
  /// Boundary table: reducer behavior for combined messages
  ///
  /// | records | ephemeral | patterns | filter_deltas    | Expected changes                     |
  /// |---------|-----------|----------|------------------|--------------------------------------|
  /// | [1]     | []        | []       | {all:+1}         | records grows, counts increment       |
  /// | []      | [1]       | []       | {}               | ephemeral set, records unchanged      |
  /// | [1]     | [1]       | []       | {all:+1}         | both records and ephemeral update     |
  /// | []      | []        | [1]      | {}               | patterns grow, rest unchanged         |
  /// | [1]     | []        | [1]      | {all:+1,pat:+1}  | records + patterns + counts all grow  |

  it("should handle records + ephemeral + patterns + deltas in one message", () => {
    scenario(
      () => ({
        state: EMPTY_STATE,
        action: makeEnrichedMessage({
          records: [makeWireRecord({ id: "durable-1", depth: 0 })],
          ephemeral: [makeWireRecord({ id: "progress-1", record_type: "system_event" })],
          patterns: [{ type: "test.cycle", label: "Test", events: ["e1"] }],
          filter_deltas: { all: 1, tools: 1 },
        }),
      }),
      ({ state, action }) => enrichedReducer(state, action),
      (result) => {
        // Durable: accumulated
        expect(result.records).toHaveLength(1);
        expect(result.records[0]!.id).toBe("durable-1");

        // Ephemeral: set (not accumulated)
        expect(result.currentEphemeral).not.toBeNull();
        expect(result.currentEphemeral!.id).toBe("progress-1");

        // Patterns: accumulated
        expect(result.patterns).toHaveLength(1);

        // Filter counts: incremented (per-session under "s1")
        const s1 = result.filterCounts["s1"]!;
        expect(s1["all"]).toBe(1);
        expect(s1["tools"]).toBe(1);

        // Tree index: only durable
        expect(result.treeIndex.has("durable-1")).toBe(true);
        expect(result.treeIndex.has("progress-1")).toBe(false);
      },
    );
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("toPatternView — server→client pattern mapping")
// ═══════════════════════════════════════════════════════════════════

describe("toPatternView — server→client pattern mapping", () => {
  it("should map pattern_type to type, summary to label, event_ids to events", () => {
    const server: ServerPatternEvent = {
      pattern_type: "git.workflow",
      session_id: "sess-1",
      event_ids: ["e1", "e2", "e3"],
      started_at: "2025-01-10T12:00:00Z",
      ended_at: "2025-01-10T12:01:00Z",
      summary: "Git: status → add → commit",
      metadata: { commands: ["status", "add", "commit"] },
    };
    const view = toPatternView(server);
    expect(view.type).toBe("git.workflow");
    expect(view.label).toBe("Git: status → add → commit");
    expect(view.events).toEqual(["e1", "e2", "e3"]);
  });

  it("should handle empty event_ids", () => {
    const server: ServerPatternEvent = {
      pattern_type: "turn.phase",
      session_id: "sess-1",
      event_ids: [],
      started_at: "",
      ended_at: "",
      summary: "conversation",
      metadata: {},
    };
    const view = toPatternView(server);
    expect(view.events).toEqual([]);
    expect(view.type).toBe("turn.phase");
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("getFilterCounts — derived count selector")
// ═══════════════════════════════════════════════════════════════════

describe("getFilterCounts — derived count selector", () => {
  const perSession = {
    s1: { all: 100, user: 20, tools: 30 },
    s2: { all: 200, user: 50, tools: 80 },
    s3: { all: 50, user: 10 },
  };

  it("should return single session counts when sessionFilter is set", () => {
    const counts = getFilterCounts(perSession, "s1");
    expect(counts["all"]).toBe(100);
    expect(counts["user"]).toBe(20);
    expect(counts["tools"]).toBe(30);
  });

  it("should aggregate all sessions when sessionFilter is null", () => {
    const counts = getFilterCounts(perSession, null);
    expect(counts["all"]).toBe(350);
    expect(counts["user"]).toBe(80);
    expect(counts["tools"]).toBe(110);
  });

  it("should return empty object for unknown session", () => {
    const counts = getFilterCounts(perSession, "unknown");
    expect(counts).toEqual({});
  });

  it("should return empty object for empty perSession", () => {
    const counts = getFilterCounts({}, null);
    expect(counts).toEqual({});
  });
});
