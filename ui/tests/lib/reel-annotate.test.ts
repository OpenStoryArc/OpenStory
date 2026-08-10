import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  appendBeatInk,
  applyBeatInkIntent,
  beatKeyString,
  clearBeatInk,
  emptyBeatInkStore,
  getBeatInk,
  normalizeBeatInkStore,
  parseBeatKey,
  setBeatInk,
  summarizeReelInk,
} from "@/lib/reel-annotate";

describe("BeatKey", () => {
  it("round-trips string form", () => {
    const k = { reelId: "reel-abc", beatIndex: 2 };
    expect(beatKeyString(k)).toBe("reel-abc:2");
    expect(parseBeatKey("reel-abc:2")).toEqual(k);
    expect(parseBeatKey("bad")).toBeNull();
  });
});

describe("beat ink isolation", () => {
  it("stores ink 1:1 per beat — other beats untouched", () => {
    scenario(
      () => {
        let s = emptyBeatInkStore();
        const a = { reelId: "reel-1", beatIndex: 0 };
        const b = { reelId: "reel-1", beatIndex: 1 };
        s = appendBeatInk(s, a, [
          { type: "path", points: [{ x: 0.1, y: 0.1 }, { x: 0.2, y: 0.2 }], stroke: "#facc15" },
        ], { now: () => "t1" });
        s = appendBeatInk(s, b, [
          { type: "circle", cx: 0.5, cy: 0.5, r: 0.1 },
        ], { now: () => "t2" });
        s = appendBeatInk(s, a, [
          { type: "path", points: [{ x: 0.3, y: 0.3 }, { x: 0.4, y: 0.4 }], stroke: "#facc15" },
        ], { now: () => "t3" });
        return s;
      },
      (s) => s,
      (s) => {
        expect(getBeatInk(s, { reelId: "reel-1", beatIndex: 0 }).strokes).toHaveLength(2);
        expect(getBeatInk(s, { reelId: "reel-1", beatIndex: 1 }).strokes).toHaveLength(1);
        expect(getBeatInk(s, { reelId: "reel-1", beatIndex: 2 }).strokes).toHaveLength(0);
      },
    );
  });

  it("clearBeatInk only clears that slide", () => {
    let s = emptyBeatInkStore();
    const a = { reelId: "r", beatIndex: 0 };
    const b = { reelId: "r", beatIndex: 1 };
    s = setBeatInk(s, a, [{ type: "line", x1: 0, y1: 0, x2: 1, y2: 1 }]);
    s = setBeatInk(s, b, [{ type: "line", x1: 0, y1: 1, x2: 1, y2: 0 }]);
    s = clearBeatInk(s, a);
    expect(getBeatInk(s, a).strokes).toHaveLength(0);
    expect(getBeatInk(s, b).strokes).toHaveLength(1);
  });

  it("normalizeBeatInkStore rejects wrong version", () => {
    expect(normalizeBeatInkStore({ v: 99, byKey: {} }).byKey).toEqual({});
  });
});

describe("applyBeatInkIntent — agent/user parity", () => {
  it("replaces and appends per slide independently", () => {
    let s = emptyBeatInkStore();
    s = applyBeatInkIntent(s, {
      reelId: "r1",
      beatIndex: 0,
      clear: true,
      strokes: [{ type: "circle", cx: 0.2, cy: 0.2, r: 0.05 }],
    });
    s = applyBeatInkIntent(s, {
      reelId: "r1",
      beatIndex: 1,
      mode: "append",
      strokes: [{ type: "line", x1: 0, y1: 0, x2: 1, y2: 1 }],
    });
    s = applyBeatInkIntent(s, {
      reelId: "r1",
      beatIndex: 0,
      mode: "append",
      strokes: [{ type: "line", x1: 0.1, y1: 0.1, x2: 0.2, y2: 0.2 }],
    });
    expect(getBeatInk(s, { reelId: "r1", beatIndex: 0 }).strokes).toHaveLength(2);
    expect(getBeatInk(s, { reelId: "r1", beatIndex: 1 }).strokes).toHaveLength(1);
    s = applyBeatInkIntent(s, { reelId: "r1", beatIndex: 0, clear: true, strokes: [] });
    expect(getBeatInk(s, { reelId: "r1", beatIndex: 0 }).strokes).toHaveLength(0);
    expect(getBeatInk(s, { reelId: "r1", beatIndex: 1 }).strokes).toHaveLength(1);
  });

  it("summarizeReelInk lists non-empty slides for review", () => {
    let s = emptyBeatInkStore();
    s = applyBeatInkIntent(s, {
      reelId: "r1",
      beatIndex: 0,
      clear: true,
      strokes: [{ type: "circle", cx: 0.5, cy: 0.5, r: 0.1 }],
    });
    s = applyBeatInkIntent(s, {
      reelId: "r1",
      beatIndex: 2,
      clear: true,
      strokes: [
        { type: "path", points: [{ x: 0, y: 0 }, { x: 1, y: 1 }] },
        { type: "path", points: [{ x: 0, y: 1 }, { x: 1, y: 0 }] },
      ],
    });
    expect(summarizeReelInk(s, "r1")).toEqual([
      { beatIndex: 0, strokeCount: 1, empty: false },
      { beatIndex: 2, strokeCount: 2, empty: false },
    ]);
  });
});

/** Interaction matrix — declarative coverage of the problem space. */
describe("reels interaction matrix (permutations)", () => {
  const kinds = ["title", "diagram", "spotlight"] as const;
  const ops = ["annotate", "clear", "re-annotate"] as const;

  it("every kind × annotate/clear/re-annotate leaves ink only on that slide", () => {
    for (const kind of kinds) {
      void kind; // kind is narrative; ink storage is kind-agnostic (slide index)
      let s = emptyBeatInkStore();
      const reelId = `reel-${kind}`;
      for (let beat = 0; beat < 3; beat++) {
        for (const op of ops) {
          if (op === "annotate" || op === "re-annotate") {
            s = applyBeatInkIntent(s, {
              reelId,
              beatIndex: beat,
              mode: "append",
              strokes: [
                {
                  type: "path",
                  points: [
                    { x: 0.1 * (beat + 1), y: 0.1 },
                    { x: 0.2 * (beat + 1), y: 0.2 },
                  ],
                  stroke: "#facc15",
                },
              ],
            });
          } else {
            s = applyBeatInkIntent(s, {
              reelId,
              beatIndex: beat,
              clear: true,
              strokes: [],
            });
          }
        }
        // after clear + re-annotate → exactly 1 path on this beat
        expect(getBeatInk(s, { reelId, beatIndex: beat }).strokes.length).toBe(1);
      }
      // three slides independently inked
      expect(summarizeReelInk(s, reelId)).toHaveLength(3);
    }
  });

  it("player pause × jump does not merge ink across slides (storage isolation)", () => {
    // models: pause (no op on store) + jump between slides with different ink
    let s = emptyBeatInkStore();
    s = applyBeatInkIntent(s, {
      reelId: "r",
      beatIndex: 0,
      clear: true,
      strokes: [{ type: "circle", cx: 0.1, cy: 0.1, r: 0.05 }],
    });
    s = applyBeatInkIntent(s, {
      reelId: "r",
      beatIndex: 2,
      clear: true,
      strokes: [{ type: "circle", cx: 0.9, cy: 0.9, r: 0.05 }],
    });
    // "jump" to 1 — empty; 0 and 2 unchanged
    expect(getBeatInk(s, { reelId: "r", beatIndex: 1 }).strokes).toHaveLength(0);
    expect(getBeatInk(s, { reelId: "r", beatIndex: 0 }).strokes).toHaveLength(1);
    expect(getBeatInk(s, { reelId: "r", beatIndex: 2 }).strokes).toHaveLength(1);
  });
});
