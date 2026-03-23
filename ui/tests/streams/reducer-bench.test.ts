/**
 * BDD performance specs for enrichedReducer.
 *
 * The reducer is the hottest pure function in the UI pipeline.
 * Every event flows through it. These specs establish performance
 * ceilings and detect the known O(n^2) spread-copy hazard.
 *
 * Red first. Specs describe what performance SHOULD be,
 * then we measure reality against those expectations.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { enrichedReducer, getFilterCounts, EMPTY_ENRICHED_STATE, toPatternView } from "@/streams/sessions";
import type { EnrichedAction, EnrichedSessionState } from "@/streams/sessions";
import { synthInitialState, synthEnrichedStream } from "../fixtures/synth";
import type { EnrichedMessage } from "@/types/websocket";

/** Convert an EnrichedMessage (wire format) to an EnrichedAction (reducer input). */
function toAction(msg: EnrichedMessage): EnrichedAction {
  return {
    kind: "enriched",
    session_id: msg.session_id,
    records: msg.records,
    ephemeral: msg.ephemeral,
    filter_deltas: msg.filter_deltas,
    patterns: msg.patterns ? msg.patterns.map(toPatternView) : undefined,
    session_label: msg.session_label,
    session_branch: msg.session_branch,
    agent_labels: msg.agent_labels,
    total_input_tokens: msg.total_input_tokens,
    total_output_tokens: msg.total_output_tokens,
  };
}

// ═══════════════════════════════════════════════════════════════════
// Helper: time a function, return result + elapsed ms
// ═══════════════════════════════════════════════════════════════════

function timed<T>(fn: () => T): { result: T; ms: number } {
  const start = performance.now();
  const result = fn();
  return { result, ms: performance.now() - start };
}

// ═══════════════════════════════════════════════════════════════════
// 1. Cold boot — initial_state with large record sets
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — cold boot performance", () => {
  it("should process initial_state with 1,000 records in < 50ms", () =>
    scenario(
      () => synthInitialState({ count: 1_000, sessions: 1, seed: 100 }),
      (msg) => {
        const action: EnrichedAction = {
          kind: "initial_state",
          records: msg.records,
          patterns: [],
          filterCounts: msg.filter_counts,
          sessionLabels: {},
          agentLabels: {},
        };
        return timed(() => enrichedReducer(EMPTY_ENRICHED_STATE, action));
      },
      ({ result, ms }) => {
        expect(result.records).toHaveLength(1_000);
        expect(ms).toBeLessThan(50);
      },
    ));

  it("should process initial_state with 10,000 records in < 200ms", () =>
    scenario(
      () => synthInitialState({ count: 10_000, sessions: 5, seed: 200 }),
      (msg) => {
        const action: EnrichedAction = {
          kind: "initial_state",
          records: msg.records,
          patterns: [],
          filterCounts: msg.filter_counts,
          sessionLabels: {},
          agentLabels: {},
        };
        return timed(() => enrichedReducer(EMPTY_ENRICHED_STATE, action));
      },
      ({ result, ms }) => {
        expect(result.records).toHaveLength(10_000);
        expect(result.treeIndex.size).toBe(10_000);
        expect(ms).toBeLessThan(200);
      },
    ));

  it("should process initial_state with 100,000 records in < 2s", () =>
    scenario(
      () => synthInitialState({ count: 100_000, sessions: 10, seed: 300 }),
      (msg) => {
        const action: EnrichedAction = {
          kind: "initial_state",
          records: msg.records,
          patterns: [],
          filterCounts: msg.filter_counts,
          sessionLabels: {},
          agentLabels: {},
        };
        return timed(() => enrichedReducer(EMPTY_ENRICHED_STATE, action));
      },
      ({ result, ms }) => {
        expect(result.records).toHaveLength(100_000);
        expect(ms).toBeLessThan(2000);
      },
    ));
});

// ═══════════════════════════════════════════════════════════════════
// 2. Incremental append — the O(n^2) spread-copy hazard
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — incremental append performance", () => {
  it("should append 1,000 single-record batches with amortized < 1ms each", () => {
    const stream = synthEnrichedStream({
      batches: 1_000,
      recordsPerBatch: 1,
      sessions: 1,
      seed: 400,
    });

    let state: EnrichedSessionState = EMPTY_ENRICHED_STATE;
    const { ms } = timed(() => {
      for (const msg of stream) {
        state = enrichedReducer(state, toAction(msg));
      }
    });

    expect(state.records).toHaveLength(1_000);
    const amortized = ms / 1_000;
    // Each append should be fast — if this fails, the O(n^2) spread is biting
    expect(amortized).toBeLessThan(1);
  });

  it("should append 10,000 single-record batches with amortized < 2ms each", () => {
    const stream = synthEnrichedStream({
      batches: 10_000,
      recordsPerBatch: 1,
      sessions: 3,
      seed: 500,
    });

    let state: EnrichedSessionState = EMPTY_ENRICHED_STATE;
    const { ms } = timed(() => {
      for (const msg of stream) {
        state = enrichedReducer(state, toAction(msg));
      }
    });

    expect(state.records).toHaveLength(10_000);
    const amortized = ms / 10_000;
    expect(amortized).toBeLessThan(2);
  });

  it("should monitor scaling: 10K append per-item cost ratio (known O(n) copy)", () => {
    // 1K baseline
    const stream1K = synthEnrichedStream({ batches: 1_000, recordsPerBatch: 1, sessions: 1, seed: 600 });
    let state1K: EnrichedSessionState = EMPTY_ENRICHED_STATE;
    const { ms: ms1K } = timed(() => {
      for (const msg of stream1K) state1K = enrichedReducer(state1K, toAction(msg));
    });

    // 10K measurement
    const stream10K = synthEnrichedStream({ batches: 10_000, recordsPerBatch: 1, sessions: 1, seed: 700 });
    let state10K: EnrichedSessionState = EMPTY_ENRICHED_STATE;
    const { ms: ms10K } = timed(() => {
      for (const msg of stream10K) state10K = enrichedReducer(state10K, toAction(msg));
    });

    const perItem1K = ms1K / 1_000;
    const perItem10K = ms10K / 10_000;
    const ratio = perItem10K / Math.max(perItem1K, 0.001);

    // Known: concat() is O(n) per call, so single-record appends are
    // O(n^2) total. At 10x records, per-item cost grows ~10x.
    // Server batches mitigate this (fewer, larger appends).
    // Threshold: allow up to 15x. If it gets worse, something regressed.
    // TODO: Story 042 Phase 2 — persistent data structure for O(1) append.
    console.log(`Scaling monitor: 1K=${perItem1K.toFixed(3)}ms/item, 10K=${perItem10K.toFixed(3)}ms/item, ratio=${ratio.toFixed(1)}x`);
    expect(ratio).toBeLessThan(15);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 3. getFilterCounts — aggregation performance
// ═══════════════════════════════════════════════════════════════════

describe("getFilterCounts — aggregation performance", () => {
  it("should aggregate 50 sessions in < 5ms", () =>
    scenario(
      () => {
        // Build per-session filter counts for 50 sessions
        const perSession: Record<string, Record<string, number>> = {};
        for (let i = 0; i < 50; i++) {
          const sc: Record<string, number> = {};
          for (let f = 0; f < 17; f++) sc[`filter_${f}`] = Math.floor(Math.random() * 1000);
          perSession[`session-${i}`] = sc;
        }
        return perSession;
      },
      (perSession) => timed(() => getFilterCounts(perSession, null)),
      ({ result, ms }) => {
        expect(Object.keys(result).length).toBeGreaterThan(0);
        expect(ms).toBeLessThan(5);
      },
    ));

  it("should return single session counts in < 1ms", () =>
    scenario(
      () => {
        const perSession: Record<string, Record<string, number>> = {};
        for (let i = 0; i < 50; i++) {
          const sc: Record<string, number> = {};
          for (let f = 0; f < 17; f++) sc[`filter_${f}`] = Math.floor(Math.random() * 1000);
          perSession[`session-${i}`] = sc;
        }
        return perSession;
      },
      (perSession) => timed(() => getFilterCounts(perSession, "session-25")),
      ({ ms }) => expect(ms).toBeLessThan(1),
    ));
});

// ═══════════════════════════════════════════════════════════════════
// 4. Correctness under volume — does the reducer produce right state?
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — correctness at scale", () => {
  it("should preserve all records after 5,000 incremental appends", () => {
    const stream = synthEnrichedStream({
      batches: 5_000,
      recordsPerBatch: 1,
      sessions: 5,
      seed: 800,
    });

    let state: EnrichedSessionState = EMPTY_ENRICHED_STATE;
    for (const msg of stream) {
      state = enrichedReducer(state, toAction(msg));
    }

    expect(state.records).toHaveLength(5_000);
    // Every record ID should be unique
    const ids = new Set(state.records.map((r) => r.id));
    expect(ids.size).toBe(5_000);
  });

  it("should maintain correct treeIndex after 5,000 appends", () => {
    const stream = synthEnrichedStream({
      batches: 5_000,
      recordsPerBatch: 1,
      sessions: 2,
      seed: 900,
    });

    let state: EnrichedSessionState = EMPTY_ENRICHED_STATE;
    for (const msg of stream) {
      state = enrichedReducer(state, toAction(msg));
    }

    // Every record should be in the tree index
    expect(state.treeIndex.size).toBe(5_000);
    for (const r of state.records) {
      const entry = state.treeIndex.get(r.id);
      expect(entry).toBeDefined();
      expect(entry!.depth).toBe(r.depth);
    }
  });

  it("should accumulate filter deltas correctly across sessions", () => {
    const stream = synthEnrichedStream({
      batches: 100,
      recordsPerBatch: 5,
      sessions: 3,
      seed: 1000,
    });

    let state: EnrichedSessionState = EMPTY_ENRICHED_STATE;
    for (const msg of stream) {
      state = enrichedReducer(state, toAction(msg));
    }

    // Total records = 500
    expect(state.records).toHaveLength(500);

    // Sum of "all" filter across sessions should equal total records
    const allCounts = getFilterCounts(state.filterCounts, null);
    expect(allCounts["all"]).toBe(500);
  });
});
