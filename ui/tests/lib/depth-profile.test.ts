import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { sampleDepthProfile } from "@/lib/depth-profile";

// ── Boundary table for sampleDepthProfile ──────────────────────────
// Covers: empty, single, identity (<=buckets), arc shape, flat, all-zero

const BOUNDARY_TABLE: [
  string,      // description
  number[],    // input depths
  number,      // buckets
  number[],    // expected profile
][] = [
  // Empty input → empty output
  ["empty array", [], 40, []],

  // Single element → single bucket
  ["single element", [5], 40, [5]],

  // Fewer elements than buckets → identity (one per bucket)
  ["identity when len <= buckets", [0, 3, 7, 2], 40, [0, 3, 7, 2]],

  // Exactly equal to buckets → identity
  ["exact bucket count", [1, 2, 3], 3, [1, 2, 3]],

  // Arc shape (ramp up, plateau, ramp down) — 8 depths into 4 buckets
  // Buckets: [0,1] → max 1, [2,3] → max 5, [4,5] → max 5, [6,7] → max 1
  ["arc shape downsampled", [0, 1, 3, 5, 5, 4, 1, 0], 4, [1, 5, 5, 1]],

  // Flat profile — all same depth
  ["flat profile", [3, 3, 3, 3, 3, 3], 3, [3, 3, 3]],

  // All zeros
  ["all zeros", [0, 0, 0, 0], 2, [0, 0]],

  // Large spike in one bucket
  ["spike in middle", [0, 0, 100, 0, 0, 0], 3, [0, 100, 0]],

  // Non-uniform bucket sizes (7 items into 3 buckets)
  // floor: [0,2)→max(1,2)=2, [2,4)→max(3,4)=4, [4,7)→max(5,6,7)=7
  ["non-uniform bucket sizes", [1, 2, 3, 4, 5, 6, 7], 3, [2, 4, 7]],
];

describe("sampleDepthProfile — boundary table", () => {
  it.each(BOUNDARY_TABLE)(
    "%s",
    (_desc, depths, buckets, expected) => {
      scenario(
        () => ({ depths, buckets }),
        ({ depths, buckets }) => sampleDepthProfile(depths, buckets),
        (result) => expect(result).toEqual(expected),
      );
    },
  );
});
