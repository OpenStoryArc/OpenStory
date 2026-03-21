//! Spec: Story 046 — Focus auto-clears when focused event is filtered out.
//!
//! When a user focuses on a subtree then switches filters such that
//! the focused event is no longer visible, focusRootId should auto-clear.

import { describe, it, expect } from "vitest";
import { shouldClearFocus } from "@/lib/focus";

describe("focus auto-clear", () => {
  it("should clear focus when focusRootId is not in visible row IDs", () => {
    const visibleIds = new Set(["r1", "r2", "r3"]);
    expect(shouldClearFocus("r4", visibleIds)).toBe(true);
  });

  it("should NOT clear focus when focusRootId is in visible row IDs", () => {
    const visibleIds = new Set(["r1", "r2", "r3"]);
    expect(shouldClearFocus("r2", visibleIds)).toBe(false);
  });

  it("should NOT clear focus when focusRootId is null", () => {
    const visibleIds = new Set(["r1", "r2"]);
    expect(shouldClearFocus(null, visibleIds)).toBe(false);
  });

  it("should clear focus when visible set is empty", () => {
    const visibleIds = new Set<string>();
    expect(shouldClearFocus("r1", visibleIds)).toBe(true);
  });
});
