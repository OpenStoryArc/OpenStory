import { describe, it, expect } from "vitest";
import { summarizePatterns, patternRollupLabel } from "@/lib/pattern-rollup";

const P = (type: string) => ({ type });

/** A session row showed a WALL of raw detector pills (eval_apply.eval/apply/
 *  turn_end, turn.sentence ×30). The rollup collapses that to a calm signal:
 *  completed eval→apply CYCLES (turn_end) and SENTENCES (turn.sentence), folding
 *  the eval/apply sub-steps into their cycle. */
describe("summarizePatterns — calm rollup of the pattern wall", () => {
  it("counts turn_end as cycles and turn.sentence as sentences", () => {
    const r = summarizePatterns([
      P("eval_apply.eval"), P("eval_apply.apply"), P("eval_apply.turn_end"),
      P("eval_apply.eval"), P("eval_apply.apply"), P("eval_apply.turn_end"),
      P("turn.sentence"), P("turn.sentence"),
    ]);
    expect(r.cycles).toBe(2);
    expect(r.sentences).toBe(2);
    expect(r.other).toBe(0);
    expect(r.total).toBe(8); // raw count preserved for the expand toggle
  });

  it("folds eval/apply sub-steps (they don't count as their own thing)", () => {
    const r = summarizePatterns([P("eval_apply.eval"), P("eval_apply.apply")]);
    expect(r.cycles).toBe(0);
    expect(r.sentences).toBe(0);
    expect(r.other).toBe(0);
    expect(r.total).toBe(2);
  });

  it("counts genuinely-other pattern types under other", () => {
    const r = summarizePatterns([P("plan.created"), P("eval_apply.turn_end")]);
    expect(r.cycles).toBe(1);
    expect(r.other).toBe(1);
  });

  it("empty → all zero", () => {
    expect(summarizePatterns([])).toEqual({ cycles: 0, sentences: 0, other: 0, total: 0 });
  });
});

describe("patternRollupLabel — a one-line calm summary", () => {
  it("pluralizes and joins present parts only", () => {
    expect(patternRollupLabel({ cycles: 12, sentences: 8, other: 0, total: 20 })).toBe("12 cycles · 8 sentences");
    expect(patternRollupLabel({ cycles: 1, sentences: 1, other: 0, total: 2 })).toBe("1 cycle · 1 sentence");
  });
  it("shows other when present, hides zero parts", () => {
    expect(patternRollupLabel({ cycles: 3, sentences: 0, other: 2, total: 5 })).toBe("3 cycles · 2 other");
    expect(patternRollupLabel({ cycles: 0, sentences: 0, other: 0, total: 0 })).toBe("");
  });
});
