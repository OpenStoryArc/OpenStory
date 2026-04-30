/**
 * BDD performance specs for enrichedReducer.
 *
 * The reducer is the hottest pure function in the UI pipeline. Every
 * record that enters state goes through it. These specs establish
 * performance ceilings.
 *
 * After feat/lazy-load-initial-state, cold-boot costs are dominated by
 * `session_records_loaded` (REST seed) instead of `initial_state` —
 * the handshake is sidebar-only now. The benchmarks were rewritten to
 * exercise the new shape.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { enrichedReducer, EMPTY_ENRICHED_STATE, toPatternView } from "@/streams/sessions";
import type { EnrichedAction, EnrichedSessionState } from "@/streams/sessions";
import { synthBatch, synthEnrichedStream } from "../fixtures/synth";
import type { EnrichedMessage } from "@/types/websocket";

/** Convert an EnrichedMessage (wire format) to an EnrichedAction (reducer input). */
function toAction(msg: EnrichedMessage): EnrichedAction {
  return {
    kind: "enriched",
    session_id: msg.session_id,
    records: msg.records,
    ephemeral: msg.ephemeral,
    patterns: msg.patterns ? msg.patterns.map(toPatternView) : undefined,
    session_label: msg.session_label,
    session_branch: msg.session_branch,
    total_input_tokens: msg.total_input_tokens,
    total_output_tokens: msg.total_output_tokens,
  };
}

function timed<T>(fn: () => T): { result: T; ms: number } {
  const start = performance.now();
  const result = fn();
  return { result, ms: performance.now() - start };
}

// ═══════════════════════════════════════════════════════════════════
// 1. Cold boot — session_records_loaded with large record sets
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — cold boot performance", () => {
  it("should process session_records_loaded with 1,000 records in < 50ms", () =>
    scenario(
      () => {
        const records = synthBatch({ count: 1000, sessions: 1, seed: 42 });
        const action: EnrichedAction = {
          kind: "session_records_loaded",
          session_id: records[0]?.session_id ?? "synth-42-s0",
          records,
        };
        return { state: EMPTY_ENRICHED_STATE, action };
      },
      ({ state, action }) => timed(() => enrichedReducer(state, action)),
      ({ result, ms }) => {
        expect(result.records).toHaveLength(1000);
        expect(ms).toBeLessThan(50);
      },
    ));

  it("should process session_records_loaded with 5,000 records in < 200ms", () =>
    scenario(
      () => {
        const records = synthBatch({ count: 5000, sessions: 3, seed: 42 });
        const action: EnrichedAction = {
          kind: "session_records_loaded",
          session_id: records[0]?.session_id ?? "synth-42-s0",
          records,
        };
        return { state: EMPTY_ENRICHED_STATE, action };
      },
      ({ state, action }) => timed(() => enrichedReducer(state, action)),
      ({ result, ms }) => {
        expect(result.records).toHaveLength(5000);
        expect(ms).toBeLessThan(200);
      },
    ));

  it("should process session_records_loaded with 10,000 records in < 500ms", () =>
    scenario(
      () => {
        const records = synthBatch({ count: 10000, sessions: 5, seed: 42 });
        const action: EnrichedAction = {
          kind: "session_records_loaded",
          session_id: records[0]?.session_id ?? "synth-42-s0",
          records,
        };
        return { state: EMPTY_ENRICHED_STATE, action };
      },
      ({ state, action }) => timed(() => enrichedReducer(state, action)),
      ({ result, ms }) => {
        expect(result.records).toHaveLength(10000);
        expect(ms).toBeLessThan(500);
      },
    ));
});

// ═══════════════════════════════════════════════════════════════════
// 2. Streaming — enriched messages append + dedup
// ═══════════════════════════════════════════════════════════════════

describe("enrichedReducer — streaming throughput", () => {
  it("should fold 100 enriched messages with 10 records each in < 100ms", () =>
    scenario(
      () => synthEnrichedStream({
        batches: 100,
        recordsPerBatch: 10,
        sessions: 1,
        seed: 42,
      }),
      (messages) =>
        timed(() => {
          let state: EnrichedSessionState = EMPTY_ENRICHED_STATE;
          for (const msg of messages) state = enrichedReducer(state, toAction(msg));
          return state;
        }),
      ({ result, ms }) => {
        expect(result.records).toHaveLength(1000);
        expect(ms).toBeLessThan(100);
      },
    ));

  it("should fold 500 enriched messages with 5 records each in < 250ms", () =>
    scenario(
      () => synthEnrichedStream({
        batches: 500,
        recordsPerBatch: 5,
        sessions: 1,
        seed: 42,
      }),
      (messages) =>
        timed(() => {
          let state: EnrichedSessionState = EMPTY_ENRICHED_STATE;
          for (const msg of messages) state = enrichedReducer(state, toAction(msg));
          return state;
        }),
      ({ result, ms }) => {
        expect(result.records).toHaveLength(2500);
        expect(ms).toBeLessThan(250);
      },
    ));
});
