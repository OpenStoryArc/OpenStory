/**
 * BDD performance specs for toTimelineRows and filter predicates.
 *
 * toTimelineRows is a pure function that runs on every state change.
 * It sorts and transforms. At scale, it's the second hottest path
 * after the reducer.
 *
 * Filter predicates run client-side for instant switching.
 * Some use regex, some use string search. We need to know
 * which ones are slow at 100K records.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { toTimelineRows } from "@/lib/timeline";
import { TIMELINE_FILTERS } from "@/lib/timeline-filters";
import { synthBatch } from "../fixtures/synth";

// ═══════════════════════════════════════════════════════════════════
// Helper: time a function
// ═══════════════════════════════════════════════════════════════════

function timed<T>(fn: () => T): { result: T; ms: number } {
  const start = performance.now();
  const result = fn();
  return { result, ms: performance.now() - start };
}

// ═══════════════════════════════════════════════════════════════════
// 1. toTimelineRows — sort + transform performance
// ═══════════════════════════════════════════════════════════════════

describe("toTimelineRows — performance", () => {
  it("should transform 1,000 records in < 15ms", () =>
    scenario(
      () => synthBatch({ count: 1_000, sessions: 1, seed: 10 }),
      (records) => timed(() => toTimelineRows(records)),
      ({ result, ms }) => {
        expect(result.length).toBeGreaterThan(0);
        expect(ms).toBeLessThan(15);
      },
    ));

  it("should transform 10,000 records in < 100ms", () =>
    scenario(
      () => synthBatch({ count: 10_000, sessions: 3, seed: 20 }),
      (records) => timed(() => toTimelineRows(records)),
      ({ result, ms }) => {
        expect(result.length).toBeGreaterThan(0);
        expect(ms).toBeLessThan(100);
      },
    ));

  it("should transform 100,000 records in < 1s", () =>
    scenario(
      () => synthBatch({ count: 100_000, sessions: 10, seed: 30 }),
      (records) => timed(() => toTimelineRows(records)),
      ({ result, ms }) => {
        expect(result.length).toBeGreaterThan(0);
        expect(ms).toBeLessThan(1000);
      },
    ));

  it("should produce rows sorted newest-first", () =>
    scenario(
      () => synthBatch({ count: 500, sessions: 2, seed: 40 }),
      (records) => toTimelineRows(records),
      (rows) => {
        for (let i = 1; i < rows.length; i++) {
          expect(rows[i]!.timestamp <= rows[i - 1]!.timestamp).toBe(true);
        }
      },
    ));

  it("should filter out SKIP_TYPES (token_usage, session_meta, etc.)", () =>
    scenario(
      () => synthBatch({ count: 1_000, sessions: 1, seed: 50 }),
      (records) => ({
        rows: toTimelineRows(records),
        inputWithSkipped: records.filter((r) =>
          ["token_usage", "session_meta", "turn_start", "file_snapshot", "context_compaction"].includes(r.record_type),
        ).length,
      }),
      ({ rows, inputWithSkipped }) => {
        // If there were skippable records, output should be smaller than input
        if (inputWithSkipped > 0) {
          expect(rows.length).toBeLessThan(1_000);
        }
      },
    ));

  it("should not degrade more than linearly: 100K should not be 100x slower than 1K per-item", () => {
    const records1K = synthBatch({ count: 1_000, sessions: 1, seed: 60 });
    const records100K = synthBatch({ count: 100_000, sessions: 1, seed: 70 });

    const { ms: ms1K } = timed(() => toTimelineRows(records1K));
    const { ms: ms100K } = timed(() => toTimelineRows(records100K));

    const perItem1K = ms1K / 1_000;
    const perItem100K = ms100K / 100_000;
    const ratio = perItem100K / Math.max(perItem1K, 0.001); // avoid div by 0

    // Sort is O(n log n), so per-item cost grows logarithmically.
    // At 100x records, expect ~7x per-item cost (log2(100K)/log2(1K) ≈ 1.7).
    // Allow up to 10x for overhead.
    console.log(`Timeline scaling: 1K=${ms1K.toFixed(1)}ms, 100K=${ms100K.toFixed(1)}ms, ratio=${ratio.toFixed(1)}x per-item`);
    expect(ratio).toBeLessThan(10);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 2. Filter predicates — per-filter performance profiling
// ═══════════════════════════════════════════════════════════════════

describe("TIMELINE_FILTERS — predicate performance", () => {
  // Generate a single large dataset shared across filter benchmarks
  const records = synthBatch({ count: 100_000, sessions: 10, seed: 80 });

  // Boundary table: each filter, its time budget, and match expectation
  const filterBudgets: Array<{ name: string; maxMs: number }> = [
    { name: "all", maxMs: 20 },
    { name: "conversation", maxMs: 50 },
    { name: "code", maxMs: 50 },
    { name: "commands", maxMs: 50 },
    { name: "tests", maxMs: 150 },
    { name: "git", maxMs: 100 },
    { name: "errors", maxMs: 100 },
    { name: "thinking", maxMs: 50 },
    { name: "agents", maxMs: 50 },
  ];

  for (const { name, maxMs } of filterBudgets) {
    it(`filter "${name}" should scan 100K records in < ${maxMs}ms`, () => {
      const pred = TIMELINE_FILTERS[name];
      expect(pred).toBeDefined();

      const { result, ms } = timed(() => {
        let count = 0;
        for (const r of records) {
          if (pred!(r)) count++;
        }
        return count;
      });

      console.log(`  filter "${name}": ${result} matches in ${ms.toFixed(1)}ms`);
      expect(ms).toBeLessThan(maxMs);
    });
  }

  it("all 9 filters scanning 100K records total should complete in < 1s", () => {
    const { ms } = timed(() => {
      for (const [, pred] of Object.entries(TIMELINE_FILTERS)) {
        for (const r of records) {
          pred(r);
        }
      }
    });

    console.log(`All 9 filters x 100K records = ${ms.toFixed(0)}ms`);
    expect(ms).toBeLessThan(1000);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 3. Combined pipeline: generate → reduce → transform → filter
// ═══════════════════════════════════════════════════════════════════

describe("full pipeline — generate → reduce → toTimelineRows → filter", () => {
  it("should process 10,000 events through full pipeline in < 500ms", () => {
    const { ms } = timed(() => {
      // 1. Generate
      const records = synthBatch({ count: 10_000, sessions: 3, seed: 90 });
      // 2. Transform to timeline rows
      const rows = toTimelineRows(records);
      // 3. Apply each filter
      for (const [, pred] of Object.entries(TIMELINE_FILTERS)) {
        rows.filter((row) => pred(row.record as any));
      }
      return rows.length;
    });

    console.log(`Full pipeline 10K: ${ms.toFixed(0)}ms`);
    expect(ms).toBeLessThan(500);
  });
});
