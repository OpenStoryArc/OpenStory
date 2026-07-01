import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { pointJitter } from "@/lib/sessions-scatter";

describe("pointJitter", () => {
  it("is deterministic for a given id", () => {
    scenario(
      () => [pointJitter("abc123", 5), pointJitter("abc123", 5)],
      ([a, b]) => ({ a, b }),
      ({ a, b }) => expect(a).toEqual(b),
    );
  });

  it("stays within the given radius (disk, not square)", () => {
    scenario(
      () => Array.from({ length: 200 }, (_, i) => pointJitter(`session-${i}`, 6)),
      (offs) => offs,
      (offs) => {
        for (const o of offs) {
          expect(Math.hypot(o.dx, o.dy)).toBeLessThanOrEqual(6 + 1e-9);
        }
      },
    );
  });

  it("spreads distinct ids to distinct offsets (declusters a pile)", () => {
    scenario(
      () => new Set(Array.from({ length: 50 }, (_, i) => `s${i}`).map((id) => `${pointJitter(id, 5).dx.toFixed(3)},${pointJitter(id, 5).dy.toFixed(3)}`)),
      (set) => set.size,
      // Pre-jitter all 50 sit at one point (size 1). Post-jitter nearly all
      // separate; the few ties are 3-decimal rounding, not identical offsets.
      (size) => expect(size).toBeGreaterThan(40),
    );
  });

  it("scales with radius (0 radius → no offset)", () => {
    scenario(
      () => pointJitter("anything", 0),
      (o) => o,
      (o) => {
        expect(o.dx).toBe(0);
        expect(o.dy).toBe(0);
      },
    );
  });
});
