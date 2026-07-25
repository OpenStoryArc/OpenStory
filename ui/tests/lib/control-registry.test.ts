import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  CONTROL_TARGETS,
  isKnownControlTarget,
  knownControlTargets,
  unknownTargetHint,
} from "@/lib/control-registry";

describe("when agent discovers control vocabulary", () => {
  it("should list registered toggle targets including canvas.mode and spotlight", () => {
    scenario(
      () => knownControlTargets(),
      (targets) => targets,
      (targets) => {
        expect(targets).toContain("canvas.mode");
        expect(targets).toContain("spotlight");
        expect(targets).toContain("story.sort");
        expect(targets).toContain("theme");
        // Stable sorted order for schema/docs consumers.
        expect(targets).toEqual([...targets].sort());
      },
    );
  });

  it("should recognize known targets and reject unknown ones", () => {
    scenario(
      () => ({
        known: isKnownControlTarget("canvas.mode"),
        unknown: isKnownControlTarget("dom.click"),
      }),
      (r) => r,
      (r) => {
        expect(r.known).toBe(true);
        expect(r.unknown).toBe(false);
      },
    );
  });

  it("should describe canvas.mode with closed enum values", () => {
    scenario(
      () => CONTROL_TARGETS["canvas.mode"],
      (d) => d,
      (d) => {
        expect(d).toBeDefined();
        if (!d) return;
        expect(d.actions).toContain("toggle");
        expect(d.values).toContain("sunburst");
        expect(d.values).toContain("heatmap");
        expect(d.description.length).toBeGreaterThan(0);
      },
    );
  });

  it("should hint that route-owned state is not a toggle target", () => {
    scenario(
      () => unknownTargetHint("detailView"),
      (hint) => hint,
      (hint) => {
        expect(hint).toContain("detailView");
        expect(hint).toMatch(/open_view|query|focus_event/);
        expect(hint).toContain("canvas.mode");
      },
    );
  });
});
