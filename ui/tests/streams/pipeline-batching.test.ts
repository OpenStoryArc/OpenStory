//! Spec: Pipeline batching — bufferTime(16ms) reduces renders from 100/s to ~60/s.
//!
//! Fix A from the CLS+FPS plan: batch WS actions into 16ms windows,
//! fold each batch through enrichedReducer, emit only final state.

import { describe, it, expect } from "vitest";
import { Subject, firstValueFrom, toArray, take } from "rxjs";

import type { WsMessage } from "@/types/websocket";
import type { WireRecord } from "@/types/wire-record";
import {
  buildSessionState$,
  EMPTY_ENRICHED_STATE,
  type EnrichedSessionState,
} from "@/streams/sessions";

// ── Helpers ──

function wire(overrides: Partial<WireRecord> = {}): WireRecord {
  return {
    id: `rec-${Math.random().toString(36).slice(2, 8)}`,
    seq: 1,
    session_id: "s1",
    timestamp: "2025-01-01T00:00:00Z",
    record_type: "tool_use",
    payload: { tool: "Bash", input: "ls" },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 10,
    ...overrides,
  } as WireRecord;
}

function enrichedMsg(records: WireRecord[], deltas: Record<string, number> = {}): WsMessage {
  return {
    kind: "enriched",
    session_id: "s1",
    records,
    ephemeral: [],
    filter_deltas: deltas,
  } as WsMessage;
}

// ═══════════════════════════════════════════════════════════════════
// describe("pipeline batching")
// ═══════════════════════════════════════════════════════════════════

describe("pipeline batching", () => {
  it("batchMs=0: emits state after each action (backward compat)", async () => {
    const ws$ = new Subject<WsMessage>();
    // batchMs=0 → no buffering, each message produces one emission
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    // 1 startWith + 2 actions = 3 emissions
    const promise = firstValueFrom(state$.pipe(take(3), toArray()));
    ws$.next(enrichedMsg([wire({ id: "r1" })]));
    ws$.next(enrichedMsg([wire({ id: "r2" })]));
    ws$.complete();

    const states = await promise;
    expect(states).toHaveLength(3);
    // First is the startWith empty state
    expect(states[0]!.records).toHaveLength(0);
    // Each subsequent action produces its own emission
    expect(states[1]!.records).toHaveLength(1);
    expect(states[2]!.records).toHaveLength(2);
  });

  it("batchMs=16: batches rapid messages into fewer emissions", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 16 });

    // Collect emissions after the startWith. Fire 10 messages synchronously —
    // they should all land in the same bufferTime window → ≤2 emissions.
    const collected: EnrichedSessionState[] = [];
    const sub = state$.subscribe((s) => collected.push(s));

    // First emission is the startWith(EMPTY)
    expect(collected).toHaveLength(1);
    expect(collected[0]).toBe(EMPTY_ENRICHED_STATE);

    // Fire 10 messages synchronously
    for (let i = 0; i < 10; i++) {
      ws$.next(enrichedMsg([wire({ id: `r${i}`, seq: i + 1 })]));
    }

    // Wait for bufferTime to flush (16ms + margin)
    await new Promise((r) => setTimeout(r, 50));

    sub.unsubscribe();

    // startWith(1) + batched emission(s) ≤ 3
    expect(collected.length).toBeGreaterThanOrEqual(2);
    expect(collected.length).toBeLessThanOrEqual(3);

    // Final state should have all 10 records
    const final = collected[collected.length - 1]!;
    expect(final.records).toHaveLength(10);
  });

  it("batchMs=16: does not emit for empty buffer windows", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 16 });

    const collected: EnrichedSessionState[] = [];
    const sub = state$.subscribe((s) => collected.push(s));

    // Wait 50ms with no messages — no extra emissions beyond startWith
    await new Promise((r) => setTimeout(r, 50));

    sub.unsubscribe();
    // Only the startWith emission
    expect(collected).toHaveLength(1);
  });

  it("batchMs=16: handles initial_state in buffer correctly", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 16 });

    const collected: EnrichedSessionState[] = [];
    const sub = state$.subscribe((s) => collected.push(s));

    // Send initial_state followed by enriched in same window
    ws$.next({
      kind: "initial_state",
      records: [wire({ id: "boot-1" }), wire({ id: "boot-2" })],
      filter_counts: { s1: { all: 2 } },
    } as WsMessage);
    ws$.next(enrichedMsg([wire({ id: "r3", seq: 3 })]));

    await new Promise((r) => setTimeout(r, 50));
    sub.unsubscribe();

    const final = collected[collected.length - 1]!;
    // initial_state resets (2 records), then enriched appends (1) = 3
    expect(final.records).toHaveLength(3);
  });

  it("batchMs=16: preserves all records from batch (no drops)", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 16 });

    const collected: EnrichedSessionState[] = [];
    const sub = state$.subscribe((s) => collected.push(s));

    // Fire 50 messages rapidly — every record must survive
    const ids: string[] = [];
    for (let i = 0; i < 50; i++) {
      const id = `rec-${i}`;
      ids.push(id);
      ws$.next(enrichedMsg([wire({ id, seq: i + 1 })]));
    }

    await new Promise((r) => setTimeout(r, 50));
    sub.unsubscribe();

    const final = collected[collected.length - 1]!;
    expect(final.records).toHaveLength(50);
    // Verify all IDs present
    const resultIds = final.records.map((r) => r.id);
    for (const id of ids) {
      expect(resultIds).toContain(id);
    }
  });

  it("batchMs=16: accumulates filter_deltas across batched actions", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 16 });

    const collected: EnrichedSessionState[] = [];
    const sub = state$.subscribe((s) => collected.push(s));

    // 5 messages each adding {tools: 1} delta
    for (let i = 0; i < 5; i++) {
      ws$.next(enrichedMsg([wire({ id: `r${i}`, seq: i + 1 })], { tools: 1 }));
    }

    await new Promise((r) => setTimeout(r, 50));
    sub.unsubscribe();

    const final = collected[collected.length - 1]!;
    // All 5 deltas should accumulate: tools = 5
    expect(final.filterCounts.s1?.tools).toBe(5);
  });
});
