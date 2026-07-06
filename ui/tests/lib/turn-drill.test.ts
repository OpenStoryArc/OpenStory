import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { turnDrillTarget } from "@/lib/story";
import type { PatternView } from "@/types/wire-record";

const turn = (events: string[]) =>
  ({ id: "t", type: "turn.sentence", session_id: "s", events, metadata: {} }) as unknown as PatternView;

/** The map principle for the Story tab: every turn drills to its SOURCE. The
 *  drill target is the turn's culminating (last) event — clicking it opens that
 *  exact event in Explore. Without this, a Story turn is a dead end. */
describe("turnDrillTarget — a Story turn's source event", () => {
  it("should be the turn's last (culminating) event", () => {
    scenario(
      () => turn(["e0", "e1", "e2"]),
      (t) => turnDrillTarget(t),
      (target) => expect(target).toBe("e2"),
    );
  });

  it("should be null for a turn with no events (nothing to drill into)", () => {
    scenario(
      () => turn([]),
      (t) => turnDrillTarget(t),
      (target) => expect(target).toBeNull(),
    );
  });
});
