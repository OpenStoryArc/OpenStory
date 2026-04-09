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
      const label = FILTER_LABELS[f];
      expect(label).toBeDefined();
      expect(label!.length).toBeGreaterThan(0);
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
      const tooltip = FILTER_TOOLTIPS[f];
      expect(tooltip).toBeDefined();
      expect(tooltip!.length).toBeGreaterThan(10);
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
//
// Pre-cleanup, this asserted a label/tooltip entry for each of the 5
// legacy pattern types. They were all retired in
// chore/cut-legacy-detectors. The PATTERN_LABELS / PATTERN_TOOLTIPS
// maps now exist as the dispatch surface for any *future* named
// pattern type that wants a friendlier display label — consumers fall
// back to the raw pattern_type string when no entry exists.
// The remaining invariant we care about: any entries that DO exist
// must be human-readable (>=3 chars), not abbreviations.

describe("pattern labels and tooltips", () => {
  it("any pattern label entries should be human-readable, not abbreviations", () => {
    // Labels should be at least 3 chars (not "err", "git", etc.)
    for (const label of Object.values(PATTERN_LABELS)) {
      expect(label.length).toBeGreaterThanOrEqual(3);
    }
  });

  it("any pattern tooltip entries should be substantive descriptions", () => {
    for (const tooltip of Object.values(PATTERN_TOOLTIPS)) {
      expect(tooltip.length).toBeGreaterThan(10);
    }
  });
});
