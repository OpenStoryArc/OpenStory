//! Spec: buildSessionState$ — enriched pipeline integration.
//!
//! Plan 038 Phase 1: The stream builder should use enrichedReducer
//! and return EnrichedSessionState with WireRecords (not ViewRecords).

import { describe, it, expect } from "vitest";
import { Subject, firstValueFrom, take, toArray } from "rxjs";

import type { WsMessage } from "@/types/websocket";
import type { WireRecord } from "@/types/wire-record";
import {
  buildSessionState$,
  type EnrichedSessionState,
} from "@/streams/sessions";

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

// ═══════════════════════════════════════════════════════════════════
// describe("buildSessionState$ with enriched messages")
// ═══════════════════════════════════════════════════════════════════

describe("buildSessionState$ with enriched messages", () => {
  it("should emit EMPTY_ENRICHED_STATE as initial value", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });
    const first = await firstValueFrom(state$);
    expect(first.records).toEqual([]);
    expect(first.currentEphemeral).toBeNull();
    expect(first.filterCounts).toEqual({});
  });

  it("should store initial_state records as WireRecords with depth and parent_uuid", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    // Skip the initial empty state, take 2 total (empty + initial_state result)
    const promise = firstValueFrom(state$.pipe(take(2), toArray()));
    ws$.next({
      kind: "initial_state",
      records: [
        makeWireRecord({ id: "r1", depth: 0, parent_uuid: null }),
        makeWireRecord({ id: "r2", seq: 2, depth: 1, parent_uuid: "r1" }),
      ],
      filter_counts: { s1: { all: 2, user: 2 } },
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    expect(final.records).toHaveLength(2);
    expect(final.records[0]!.depth).toBe(0);
    expect(final.records[1]!.depth).toBe(1);
    expect(final.records[1]!.parent_uuid).toBe("r1");
  });

  it("should append enriched durable records preserving tree metadata", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(3), toArray()));
    ws$.next({
      kind: "initial_state",
      records: [makeWireRecord({ id: "r1" })],
      filter_counts: { s1: { all: 1 } },
    });
    ws$.next({
      kind: "enriched",
      session_id: "s1",
      records: [makeWireRecord({ id: "r2", seq: 2, depth: 3, parent_uuid: "r1" })],
      ephemeral: [],
      filter_deltas: { all: 1 },
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    expect(final.records).toHaveLength(2);
    expect(final.records[1]!.id).toBe("r2");
    expect(final.records[1]!.depth).toBe(3);
    expect(final.records[1]!.parent_uuid).toBe("r1");
  });

  it("should NOT accumulate ephemeral records in state.records", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(3), toArray()));
    ws$.next({
      kind: "initial_state",
      records: [makeWireRecord({ id: "r1" })],
      filter_counts: {},
    });
    ws$.next({
      kind: "enriched",
      session_id: "s1",
      records: [],
      ephemeral: [makeWireRecord({ id: "progress-1", record_type: "system_event" })],
      filter_deltas: {},
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    // Ephemeral should NOT be in records
    expect(final.records).toHaveLength(1);
    expect(final.records[0]!.id).toBe("r1");
    // But should be in currentEphemeral
    expect(final.currentEphemeral).not.toBeNull();
    expect(final.currentEphemeral!.id).toBe("progress-1");
  });

  it("should maintain treeIndex from WireRecord depth/parent_uuid", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(2), toArray()));
    ws$.next({
      kind: "initial_state",
      records: [
        makeWireRecord({ id: "root", depth: 0, parent_uuid: null }),
        makeWireRecord({ id: "child", seq: 2, depth: 1, parent_uuid: "root" }),
        makeWireRecord({ id: "grandchild", seq: 3, depth: 2, parent_uuid: "child" }),
      ],
      filter_counts: {},
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    expect(final.treeIndex.get("root")).toEqual({ depth: 0, parent_uuid: null });
    expect(final.treeIndex.get("child")).toEqual({ depth: 1, parent_uuid: "root" });
    expect(final.treeIndex.get("grandchild")).toEqual({ depth: 2, parent_uuid: "child" });
  });

  it("should store session labels from initial_state", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(2), toArray()));
    ws$.next({
      kind: "initial_state",
      records: [makeWireRecord({ id: "r1" })],
      filter_counts: {},
      session_labels: {
        s1: { label: "Fix the login bug", branch: "feature/login" },
      },
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    expect(final.sessionLabels.s1).toEqual({ label: "Fix the login bug", branch: "feature/login" });
  });

  it("should merge session label from enriched message", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(3), toArray()));
    ws$.next({
      kind: "initial_state",
      records: [makeWireRecord({ id: "r1" })],
      filter_counts: {},
    });
    ws$.next({
      kind: "enriched",
      session_id: "s1",
      records: [makeWireRecord({ id: "r2", seq: 2 })],
      ephemeral: [],
      filter_deltas: {},
      session_label: "Fix the login bug",
      session_branch: "feature/login",
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    expect(final.sessionLabels.s1).toEqual({ label: "Fix the login bug", branch: "feature/login" });
  });

  it("should preserve agent_id and is_sidechain on WireRecords", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(2), toArray()));
    ws$.next({
      kind: "initial_state",
      records: [
        makeWireRecord({ id: "r1", agent_id: "agent-abc", is_sidechain: true }),
      ],
      filter_counts: {},
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    expect(final.records[0]!.agent_id).toBe("agent-abc");
    expect(final.records[0]!.is_sidechain).toBe(true);
  });
});
