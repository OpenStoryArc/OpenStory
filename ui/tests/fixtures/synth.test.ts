/**
 * BDD specs for the synthetic event generator.
 *
 * The generator is the foundation of all performance testing.
 * It must produce structurally valid events that pass through
 * the real UI pipeline without error.
 *
 * Red first. Every spec here was written before the implementation.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  synth,
  synthBatch,
  synthInitialState,
  synthEnrichedStream,
} from "./synth";

// ═══════════════════════════════════════════════════════════════════
// 1. Basic generation — does it produce valid WireRecords?
// ═══════════════════════════════════════════════════════════════════

describe("synth() — single record generation", () => {
  it("should produce a valid WireRecord with all required fields", () =>
    scenario(
      () => synth(),
      (record) => record,
      (r) => {
        expect(r.id).toBeTruthy();
        expect(r.seq).toBeGreaterThanOrEqual(0);
        expect(r.session_id).toBeTruthy();
        expect(r.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
        expect(r.record_type).toBeTruthy();
        expect(r.payload).toBeDefined();
        expect(typeof r.depth).toBe("number");
        expect(typeof r.truncated).toBe("boolean");
        expect(typeof r.payload_bytes).toBe("number");
      },
    ));

  it("should produce unique IDs across calls", () =>
    scenario(
      () => [synth(), synth(), synth()],
      (records) => new Set(records.map((r) => r.id)),
      (ids) => expect(ids.size).toBe(3),
    ));

  it("should accept overrides for session_id", () =>
    scenario(
      () => synth({ session_id: "test-session-42" }),
      (r) => r.session_id,
      (sid) => expect(sid).toBe("test-session-42"),
    ));

  it("should accept overrides for record_type", () =>
    scenario(
      () => synth({ record_type: "tool_call" }),
      (r) => r.record_type,
      (rt) => expect(rt).toBe("tool_call"),
    ));

  it("should produce a payload matching the record_type", () =>
    scenario(
      () => synth({ record_type: "tool_call" }),
      (r) => r.payload as Record<string, unknown>,
      (p) => {
        expect(p.call_id).toBeTruthy();
        expect(p.name).toBeTruthy();
      },
    ));

  it("should produce matching tool_result payloads", () =>
    scenario(
      () => synth({ record_type: "tool_result" }),
      (r) => r.payload as Record<string, unknown>,
      (p) => {
        expect(p.call_id).toBeTruthy();
        expect(typeof p.is_error).toBe("boolean");
      },
    ));

  it("should accept depth override", () =>
    scenario(
      () => synth({ depth: 5 }),
      (r) => r.depth,
      (d) => expect(d).toBe(5),
    ));
});

// ═══════════════════════════════════════════════════════════════════
// 2. Batch generation — determinism, structural validity, volume
// ═══════════════════════════════════════════════════════════════════

describe("synthBatch() — batch generation", () => {
  it("should produce the requested number of records", () =>
    scenario(
      () => synthBatch({ count: 100 }),
      (records) => records.length,
      (len) => expect(len).toBe(100),
    ));

  it("should produce monotonically increasing timestamps within a session", () =>
    scenario(
      () => synthBatch({ count: 50, sessions: 1 }),
      (records) =>
        records
          .filter((r) => r.session_id === records[0]!.session_id)
          .map((r) => r.timestamp),
      (ts) => {
        for (let i = 1; i < ts.length; i++) {
          expect(ts[i]! >= ts[i - 1]!).toBe(true);
        }
      },
    ));

  it("should produce contiguous seq numbers within a session", () =>
    scenario(
      () => synthBatch({ count: 50, sessions: 1 }),
      (records) => records.map((r) => r.seq),
      (seqs) => {
        for (let i = 1; i < seqs.length; i++) {
          expect(seqs[i]).toBe(seqs[i - 1]! + 1);
        }
      },
    ));

  it("should distribute records across multiple sessions", () =>
    scenario(
      () => synthBatch({ count: 100, sessions: 5 }),
      (records) => new Set(records.map((r) => r.session_id)),
      (sessions) => expect(sessions.size).toBe(5),
    ));

  it("should be deterministic with the same seed", () =>
    scenario(
      () => ({
        a: synthBatch({ count: 20, seed: 42 }),
        b: synthBatch({ count: 20, seed: 42 }),
      }),
      ({ a, b }) => ({
        aIds: a.map((r) => r.id),
        bIds: b.map((r) => r.id),
      }),
      ({ aIds, bIds }) => expect(aIds).toEqual(bIds),
    ));

  it("should produce different results with different seeds", () =>
    scenario(
      () => ({
        a: synthBatch({ count: 20, seed: 1 }),
        b: synthBatch({ count: 20, seed: 2 }),
      }),
      ({ a, b }) => a[0]!.id === b[0]!.id,
      (same) => expect(same).toBe(false),
    ));

  it("should respect record type weights", () =>
    scenario(
      () =>
        synthBatch({
          count: 1000,
          sessions: 1,
          typeWeights: { tool_call: 1, tool_result: 1 },
        }),
      (records) => {
        const types = new Set(records.map((r) => r.record_type));
        return { types, count: records.length };
      },
      ({ types, count }) => {
        expect(types.size).toBe(2);
        expect(types.has("tool_call")).toBe(true);
        expect(types.has("tool_result")).toBe(true);
        expect(count).toBe(1000);
      },
    ));
});

// ═══════════════════════════════════════════════════════════════════
// 3. Structural validity — events form valid trees
// ═══════════════════════════════════════════════════════════════════

describe("synthBatch() — structural validity", () => {
  it("tool_call records should have valid ToolCall payloads", () =>
    scenario(
      () => synthBatch({ count: 200, sessions: 1 }),
      (records) => records.filter((r) => r.record_type === "tool_call"),
      (toolCalls) => {
        expect(toolCalls.length).toBeGreaterThan(0);
        for (const tc of toolCalls) {
          const p = tc.payload as Record<string, unknown>;
          expect(p.call_id).toBeTruthy();
          expect(p.name).toBeTruthy();
        }
      },
    ));

  it("parent_uuid should reference an existing record or be null", () =>
    scenario(
      () => synthBatch({ count: 100, sessions: 1 }),
      (records) => {
        const ids = new Set(records.map((r) => r.id));
        return records.filter(
          (r) => r.parent_uuid !== null && !ids.has(r.parent_uuid),
        );
      },
      (orphans) => expect(orphans).toHaveLength(0),
    ));

  it("depth should be consistent with parent chain", () =>
    scenario(
      () => synthBatch({ count: 100, sessions: 1 }),
      (records) => {
        const byId = new Map(records.map((r) => [r.id, r]));
        const violations: string[] = [];
        for (const r of records) {
          if (r.parent_uuid === null) {
            // Root records can have any depth (first record starts at 0)
            continue;
          }
          const parent = byId.get(r.parent_uuid);
          if (parent && r.depth !== parent.depth && r.depth !== parent.depth + 1) {
            violations.push(
              `${r.id}: depth=${r.depth} but parent depth=${parent.depth}`,
            );
          }
        }
        return violations;
      },
      (violations) => expect(violations).toHaveLength(0),
    ));

  it("payload_bytes should be > 0", () =>
    scenario(
      () => synthBatch({ count: 50 }),
      (records) => records.filter((r) => r.payload_bytes <= 0),
      (bad) => expect(bad).toHaveLength(0),
    ));
});

// ═══════════════════════════════════════════════════════════════════
// 4. Volume generation — can it produce at scale?
// ═══════════════════════════════════════════════════════════════════

describe("synthBatch() — volume", () => {
  it("should generate 10,000 records in under 500ms", () => {
    const start = performance.now();
    const records = synthBatch({ count: 10_000, sessions: 3 });
    const elapsed = performance.now() - start;

    expect(records).toHaveLength(10_000);
    expect(elapsed).toBeLessThan(500);
  });

  it("should generate 100,000 records in under 3s", () => {
    const start = performance.now();
    const records = synthBatch({ count: 100_000, sessions: 10 });
    const elapsed = performance.now() - start;

    expect(records).toHaveLength(100_000);
    expect(elapsed).toBeLessThan(3000);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 5. WsMessage factories — initial_state and enriched stream
// ═══════════════════════════════════════════════════════════════════

describe("synthInitialState() — WsMessage factory", () => {
  it("should produce an initial_state message with the requested record count", () =>
    scenario(
      () => synthInitialState({ count: 50, sessions: 2 }),
      (msg) => msg,
      (msg) => {
        expect(msg.kind).toBe("initial_state");
        expect("records" in msg && msg.records).toHaveLength(50);
      },
    ));

  it("should include per-session filter_counts", () =>
    scenario(
      () => synthInitialState({ count: 100, sessions: 3 }),
      (msg) =>
        "filter_counts" in msg
          ? Object.keys(msg.filter_counts as Record<string, unknown>)
          : [],
      (sessionIds) => expect(sessionIds.length).toBe(3),
    ));
});

describe("synthEnrichedStream() — enriched message batches", () => {
  it("should produce the requested number of enriched messages", () =>
    scenario(
      () =>
        synthEnrichedStream({
          batches: 10,
          recordsPerBatch: 5,
          sessions: 2,
        }),
      (msgs) => msgs,
      (msgs) => {
        expect(msgs).toHaveLength(10);
        for (const m of msgs) {
          expect(m.kind).toBe("enriched");
          if ("records" in m) {
            expect((m as unknown as { records: unknown[] }).records).toHaveLength(5);
          }
        }
      },
    ));

  it("should distribute batches across sessions", () =>
    scenario(
      () =>
        synthEnrichedStream({
          batches: 20,
          recordsPerBatch: 3,
          sessions: 4,
        }),
      (msgs) =>
        new Set(
          msgs.map((m) =>
            "session_id" in m ? (m as { session_id: string }).session_id : "",
          ),
        ),
      (sessions) => expect(sessions.size).toBe(4),
    ));
});
