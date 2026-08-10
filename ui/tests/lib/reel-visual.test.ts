import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  diagramLabelsToStrokes,
  normalizeStopKind,
  stopRequiresEventAnchor,
} from "@/lib/reel-visual";

describe("normalizeStopKind", () => {
  it("defaults unknown to spotlight", () => {
    expect(normalizeStopKind(undefined)).toBe("spotlight");
    expect(normalizeStopKind("diagram")).toBe("diagram");
  });
});

describe("stopRequiresEventAnchor", () => {
  it("only spotlight requires history anchor", () => {
    expect(stopRequiresEventAnchor("spotlight")).toBe(true);
    expect(stopRequiresEventAnchor("diagram")).toBe(false);
    expect(stopRequiresEventAnchor("title")).toBe(false);
    expect(stopRequiresEventAnchor("image")).toBe(false);
  });
});

describe("diagramLabelsToStrokes", () => {
  it("emits boxes and text for each label", () => {
    scenario(
      () => diagramLabelsToStrokes(["Bash ×3", "Edit · auth.rs"], { title: "Journey" }),
      (s) => s,
      (s) => {
        expect(s.some((x) => x.type === "text" && x.text === "Journey")).toBe(true);
        expect(s.filter((x) => x.type === "path" && x.closed).length).toBe(2);
        expect(s.some((x) => x.type === "text" && String(x.text).includes("Bash"))).toBe(true);
      },
    );
  });

  it("handles empty labels", () => {
    const s = diagramLabelsToStrokes([]);
    expect(s.some((x) => x.type === "text")).toBe(true);
  });
});
