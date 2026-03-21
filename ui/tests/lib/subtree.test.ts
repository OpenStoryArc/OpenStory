//! Spec: Subtree membership — pure functions for tree-based filtering.
//!
//! Given a "focus root" event ID, determine which events are descendants.
//! Uses parent-chain walking with path compression for O(n) performance.
//!
//! These tests are the implementation spec. Red → green → refactor.

import { describe, it, expect } from "vitest";

import { buildParentIndex, subtreeIds } from "@/lib/subtree";

// ── Helper: minimal event-like objects ──

interface FakeEvent {
  id: string;
  parent_uuid: string | null;
}

function events(...items: [string, string | null][]): FakeEvent[] {
  return items.map(([id, parent]) => ({ id, parent_uuid: parent }));
}

// ═══════════════════════════════════════════════════════════════════
// describe("buildParentIndex")
// ═══════════════════════════════════════════════════════════════════

describe("buildParentIndex", () => {
  it("should map each event ID to its parent_uuid", () => {
    const idx = buildParentIndex(events(["a", null], ["b", "a"], ["c", "b"]));
    expect(idx.get("a")).toBeNull();
    expect(idx.get("b")).toBe("a");
    expect(idx.get("c")).toBe("b");
  });

  it("should handle empty events array", () => {
    const idx = buildParentIndex(events());
    expect(idx.size).toBe(0);
  });

  it("should handle events with null parent_uuid", () => {
    const idx = buildParentIndex(events(["orphan", null]));
    expect(idx.get("orphan")).toBeNull();
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("subtreeIds — boundary table")
// ═══════════════════════════════════════════════════════════════════

describe("subtreeIds — boundary table", () => {
  // ── Single node ──
  it("single node, focus on it → {A}", () => {
    const idx = buildParentIndex(events(["A", null]));
    expect(subtreeIds("A", idx)).toEqual(new Set(["A"]));
  });

  // ── Linear chain ──
  describe("linear chain A → B → C", () => {
    const idx = buildParentIndex(events(["A", null], ["B", "A"], ["C", "B"]));

    it("focus on root A → {A, B, C}", () => {
      expect(subtreeIds("A", idx)).toEqual(new Set(["A", "B", "C"]));
    });

    it("focus on middle B → {B, C}", () => {
      expect(subtreeIds("B", idx)).toEqual(new Set(["B", "C"]));
    });

    it("focus on leaf C → {C}", () => {
      expect(subtreeIds("C", idx)).toEqual(new Set(["C"]));
    });
  });

  // ── Branching tree ──
  describe("branching: A → {B, C}, B → D", () => {
    const idx = buildParentIndex(
      events(["A", null], ["B", "A"], ["C", "A"], ["D", "B"]),
    );

    it("focus on root A → {A, B, C, D}", () => {
      expect(subtreeIds("A", idx)).toEqual(new Set(["A", "B", "C", "D"]));
    });

    it("focus on branch B → {B, D}", () => {
      expect(subtreeIds("B", idx)).toEqual(new Set(["B", "D"]));
    });

    it("focus on branch C → {C} (sibling excluded)", () => {
      expect(subtreeIds("C", idx)).toEqual(new Set(["C"]));
    });

    it("focus on leaf D → {D}", () => {
      expect(subtreeIds("D", idx)).toEqual(new Set(["D"]));
    });
  });

  // ── Nonexistent focus root ──
  it("nonexistent focus root → {root} only", () => {
    const idx = buildParentIndex(events(["A", null], ["B", "A"]));
    expect(subtreeIds("missing", idx)).toEqual(new Set(["missing"]));
  });

  // ── Empty events ──
  it("empty events → {root} only", () => {
    const idx = buildParentIndex(events());
    expect(subtreeIds("x", idx)).toEqual(new Set(["x"]));
  });

  // ── Deep chain ──
  it("deep chain (100 nodes), focus on root → all 100", () => {
    const items: [string, string | null][] = [["n0", null]];
    for (let i = 1; i < 100; i++) {
      items.push([`n${i}`, `n${i - 1}`]);
    }
    const idx = buildParentIndex(events(...items));
    const result = subtreeIds("n0", idx);
    expect(result.size).toBe(100);
    expect(result.has("n0")).toBe(true);
    expect(result.has("n99")).toBe(true);
  });

  // ── Deep chain, focus on middle ──
  it("deep chain (100 nodes), focus on n50 → 50 descendants", () => {
    const items: [string, string | null][] = [["n0", null]];
    for (let i = 1; i < 100; i++) {
      items.push([`n${i}`, `n${i - 1}`]);
    }
    const idx = buildParentIndex(events(...items));
    const result = subtreeIds("n50", idx);
    expect(result.size).toBe(50);
    expect(result.has("n49")).toBe(false);
    expect(result.has("n50")).toBe(true);
    expect(result.has("n99")).toBe(true);
  });

  // ── Multi-session (separate roots) ──
  it("multi-session: A→B + X→Y, focus on A → {A, B}", () => {
    const idx = buildParentIndex(
      events(["A", null], ["B", "A"], ["X", null], ["Y", "X"]),
    );
    const result = subtreeIds("A", idx);
    expect(result).toEqual(new Set(["A", "B"]));
    expect(result.has("X")).toBe(false);
    expect(result.has("Y")).toBe(false);
  });

  // ── Orphan node ──
  it("orphan node (null parent), focus on it → {orphan}", () => {
    const idx = buildParentIndex(events(["orphan", null], ["other", null]));
    expect(subtreeIds("orphan", idx)).toEqual(new Set(["orphan"]));
  });

  // ── Wide tree (many children of one parent) ──
  it("wide tree: root with 10 children, focus on root → all 11", () => {
    const items: [string, string | null][] = [["root", null]];
    for (let i = 0; i < 10; i++) {
      items.push([`child${i}`, "root"]);
    }
    const idx = buildParentIndex(events(...items));
    const result = subtreeIds("root", idx);
    expect(result.size).toBe(11);
  });
});

describe("subtreeIds — orphan chain to null", () => {
  it("node with parent not in index walks to null → excluded from subtree", () => {
    // "child" points to "missing" which is NOT in the parent index.
    // The chain walks: child → missing (not in index) → null. Should be excluded.
    const idx = buildParentIndex(events(["root", null], ["child", "missing"]));
    const result = subtreeIds("root", idx);
    expect(result).toEqual(new Set(["root"]));
    expect(result.has("child")).toBe(false);
  });

  it("multiple orphan chains are all excluded", () => {
    const idx = buildParentIndex(events(
      ["root", null],
      ["a", "root"],
      ["orphan1", "missing1"],
      ["orphan2", "missing2"],
    ));
    const result = subtreeIds("root", idx);
    expect(result).toEqual(new Set(["root", "a"]));
    expect(result.has("orphan1")).toBe(false);
    expect(result.has("orphan2")).toBe(false);
  });

  it("node whose parent walks to null then path compression caches exclusion", () => {
    // Chain: d → c → b → "missing" (not in index) → null
    // All should be excluded, and path compression should cache them all
    const idx = buildParentIndex(events(
      ["root", null],
      ["b", "missing"],
      ["c", "b"],
      ["d", "c"],
    ));
    const result = subtreeIds("root", idx);
    expect(result).toEqual(new Set(["root"]));
  });
});
