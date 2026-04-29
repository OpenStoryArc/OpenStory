//! Spec: enrichedReducer — initial_state, session_records_loaded, enriched.
//!
//! After feat/lazy-load-initial-state, the reducer's contract is:
//!   - `initial_state`  → seeds patterns + sessionLabels only (sidebar data).
//!   - `session_records_loaded` → appends a session's REST-fetched records
//!                                 (deduped by id) and marks the session as
//!                                 loaded.
//!   - `enriched`       → appends live durable records, updates ephemeral
//!                         slot, accumulates patterns, and merges label
//!                         updates. Live `filter_deltas` are intentionally
//!                         dropped — `state.filterCounts` was unused by any
//!                         component and was removed.

import { describe, it, expect } from "vitest";

import type { WireRecord } from "@/types/wire-record";
import {
  enrichedReducer,
  EMPTY_ENRICHED_STATE,
  toPatternView,
  type EnrichedSessionState,
  type EnrichedAction,
} from "@/streams/sessions";
import type { ServerPatternEvent } from "@/types/websocket";

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

const EMPTY_STATE: EnrichedSessionState = EMPTY_ENRICHED_STATE;

describe("enrichedReducer — initial_state (sidebar-only)", () => {
  it("seeds session_labels from initial_state", () => {
    const action: EnrichedAction = {
      kind: "initial_state",
      patterns: [],
      sessionLabels: { s1: { label: "first prompt", branch: "main" } },
    };
    const result = enrichedReducer(EMPTY_STATE, action);
    expect(result.sessionLabels.s1?.label).toBe("first prompt");
    expect(result.sessionLabels.s1?.branch).toBe("main");
  });

  it("seeds patterns from initial_state", () => {
    const pe: ServerPatternEvent = {
      pattern_type: "turn.sentence",
      session_id: "s1",
      event_ids: ["evt-1"],
      started_at: "2025-01-01T00:00:00Z",
      ended_at: "2025-01-01T00:00:01Z",
      summary: "a sentence",
      metadata: {},
    };
    const action: EnrichedAction = {
      kind: "initial_state",
      patterns: [toPatternView(pe)],
      sessionLabels: {},
    };
    const result = enrichedReducer(EMPTY_STATE, action);
    expect(result.patterns).toHaveLength(1);
    expect(result.patterns[0]?.type).toBe("turn.sentence");
  });

  it("does not populate records (records arrive lazily via REST)", () => {
    const action: EnrichedAction = {
      kind: "initial_state",
      patterns: [],
      sessionLabels: { s1: { label: "x", branch: null } },
    };
    const result = enrichedReducer(EMPTY_STATE, action);
    expect(result.records).toHaveLength(0);
    expect(result.loadedSessions.size).toBe(0);
  });
});

describe("enrichedReducer — session_records_loaded (REST seed)", () => {
  it("appends records and marks the session as loaded", () => {
    const records = [makeWireRecord({ id: "r1" }), makeWireRecord({ id: "r2", seq: 2 })];
    const action: EnrichedAction = {
      kind: "session_records_loaded",
      session_id: "s1",
      records,
    };
    const result = enrichedReducer(EMPTY_STATE, action);
    expect(result.records).toHaveLength(2);
    expect(result.loadedSessions.has("s1")).toBe(true);
  });

  it("dedups by id when re-loading the same session", () => {
    const r = makeWireRecord({ id: "r1" });
    const first = enrichedReducer(EMPTY_STATE, {
      kind: "session_records_loaded",
      session_id: "s1",
      records: [r],
    });
    const second = enrichedReducer(first, {
      kind: "session_records_loaded",
      session_id: "s1",
      records: [r, makeWireRecord({ id: "r2", seq: 2 })],
    });
    expect(second.records).toHaveLength(2);
    expect(second.records.map((x) => x.id)).toEqual(["r1", "r2"]);
  });

  it("populates treeIndex from loaded records", () => {
    const action: EnrichedAction = {
      kind: "session_records_loaded",
      session_id: "s1",
      records: [
        makeWireRecord({ id: "r1", depth: 0, parent_uuid: null }),
        makeWireRecord({ id: "r2", depth: 1, parent_uuid: "r1", seq: 2 }),
      ],
    };
    const result = enrichedReducer(EMPTY_STATE, action);
    expect(result.treeIndex.get("r1")).toEqual({ depth: 0, parent_uuid: null });
    expect(result.treeIndex.get("r2")).toEqual({ depth: 1, parent_uuid: "r1" });
  });
});

describe("enrichedReducer — enriched (live deltas)", () => {
  it("appends durable records to the flat array", () => {
    const action: EnrichedAction = {
      kind: "enriched",
      session_id: "s1",
      records: [makeWireRecord({ id: "r1" })],
      ephemeral: [],
    };
    const result = enrichedReducer(EMPTY_STATE, action);
    expect(result.records).toHaveLength(1);
    expect(result.records[0]?.id).toBe("r1");
  });

  it("dedups durable records against records already in state", () => {
    const r = makeWireRecord({ id: "r1" });
    const seeded = enrichedReducer(EMPTY_STATE, {
      kind: "session_records_loaded",
      session_id: "s1",
      records: [r],
    });
    const result = enrichedReducer(seeded, {
      kind: "enriched",
      session_id: "s1",
      records: [r],
      ephemeral: [],
    });
    expect(result.records).toHaveLength(1);
  });

  it("sets currentEphemeral to the last ephemeral record (overwrite)", () => {
    const a = makeWireRecord({ id: "eph-1", record_type: "system_event" });
    const b = makeWireRecord({ id: "eph-2", record_type: "system_event" });
    const action: EnrichedAction = {
      kind: "enriched",
      session_id: "s1",
      records: [],
      ephemeral: [a, b],
    };
    const result = enrichedReducer(EMPTY_STATE, action);
    expect(result.currentEphemeral?.id).toBe("eph-2");
  });

  it("merges session_label and token totals onto sessionLabels", () => {
    const action: EnrichedAction = {
      kind: "enriched",
      session_id: "s1",
      records: [],
      ephemeral: [],
      session_label: "fresh label",
      total_input_tokens: 100,
      total_output_tokens: 50,
    };
    const result = enrichedReducer(EMPTY_STATE, action);
    expect(result.sessionLabels.s1?.label).toBe("fresh label");
    expect(result.sessionLabels.s1?.total_input_tokens).toBe(100);
    expect(result.sessionLabels.s1?.total_output_tokens).toBe(50);
  });

  it("accumulates patterns across enriched messages", () => {
    const pe = (id: string): ServerPatternEvent => ({
      pattern_type: "turn.sentence",
      session_id: "s1",
      event_ids: [id],
      started_at: "",
      ended_at: "",
      summary: id,
      metadata: {},
    });
    const a1: EnrichedAction = {
      kind: "enriched",
      session_id: "s1",
      records: [],
      ephemeral: [],
      patterns: [toPatternView(pe("a"))],
    };
    const a2: EnrichedAction = {
      kind: "enriched",
      session_id: "s1",
      records: [],
      ephemeral: [],
      patterns: [toPatternView(pe("b"))],
    };
    const r1 = enrichedReducer(EMPTY_STATE, a1);
    const r2 = enrichedReducer(r1, a2);
    expect(r2.patterns).toHaveLength(2);
  });
});
