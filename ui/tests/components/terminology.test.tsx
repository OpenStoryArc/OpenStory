//! Spec: Story 043 — Terminology cleanup & tooltips.
//!
//! Verifies that user-facing labels use plain language instead of jargon,
//! and that badges/buttons have descriptive tooltips.

import { describe, it, expect } from "vitest";
import { FILTER_GROUPS } from "@/lib/timeline-filters";
import {
  FILTER_LABELS,
  FILTER_TOOLTIPS,
  PATTERN_LABELS,
  PATTERN_TOOLTIPS,
} from "@/lib/ui-labels";

// ═══════════════════════════════════════════════════════════════════
// describe("filter display labels — no jargon")
// ═══════════════════════════════════════════════════════════════════

describe("filter display labels — no jargon", () => {
  it("should label 'conversation' as 'Conversation'", () => {
    expect(FILTER_LABELS["conversation"]).toBe("Conversation");
  });

  it("should label 'agents' as 'Agents'", () => {
    expect(FILTER_LABELS["agents"]).toBe("Agents");
  });

  it("should include 'commands' filter in FILTER_GROUPS", () => {
    const group = FILTER_GROUPS.find((g) =>
      g.filters.includes("commands"),
    );
    expect(group).toBeDefined();
  });

  it("should have a label for every filter in FILTER_GROUPS", () => {
    const allFilters = FILTER_GROUPS.flatMap((g) => g.filters);
    for (const f of allFilters) {
      expect(FILTER_LABELS[f]).toBeDefined();
      expect(FILTER_LABELS[f].length).toBeGreaterThan(0);
    }
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("filter tooltips — every filter has a description")
// ═══════════════════════════════════════════════════════════════════

describe("filter tooltips — every filter has a description", () => {
  it("should have a tooltip for every filter in FILTER_GROUPS", () => {
    const allFilters = FILTER_GROUPS.flatMap((g) => g.filters);
    for (const f of allFilters) {
      expect(FILTER_TOOLTIPS[f]).toBeDefined();
      expect(FILTER_TOOLTIPS[f].length).toBeGreaterThan(10);
    }
  });

  it("should not use jargon in tooltips", () => {
    for (const tooltip of Object.values(FILTER_TOOLTIPS)) {
      // These terms are too internal for tooltips
      expect(tooltip.toLowerCase()).not.toContain("wire");
      expect(tooltip.toLowerCase()).not.toContain("predicate");
      expect(tooltip.toLowerCase()).not.toContain("reducer");
    }
  });
});

// ═══════════════════════════════════════════════════════════════════
// describe("pattern labels and tooltips")
// ═══════════════════════════════════════════════════════════════════

describe("pattern labels and tooltips", () => {
  const EXPECTED_PATTERNS = [
    "test.cycle",
    "git.workflow",
    "error.recovery",
    "agent.delegation",
    "turn.phase",
  ];

  it("should have a display label for every pattern type", () => {
    for (const p of EXPECTED_PATTERNS) {
      expect(PATTERN_LABELS[p]).toBeDefined();
      expect(PATTERN_LABELS[p].length).toBeGreaterThan(0);
    }
  });

  it("should have a tooltip for every pattern type", () => {
    for (const p of EXPECTED_PATTERNS) {
      expect(PATTERN_TOOLTIPS[p]).toBeDefined();
      expect(PATTERN_TOOLTIPS[p].length).toBeGreaterThan(10);
    }
  });

  it("pattern labels should be human-readable, not abbreviations", () => {
    // Labels should be at least 3 chars (not "err", "git", etc.)
    for (const label of Object.values(PATTERN_LABELS)) {
      expect(label.length).toBeGreaterThanOrEqual(3);
    }
  });
});
