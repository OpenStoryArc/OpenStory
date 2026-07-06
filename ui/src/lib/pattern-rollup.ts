/** Collapse the raw pattern-detector wall into a calm signal.
 *
 *  Sessions carry many streaming patterns — eval_apply.eval / .apply / .turn_end
 *  and turn.sentence — which, rendered one-pill-each, become a wall of internal
 *  machinery (30+ near-identical badges). A human reads "cycles" and "sentences",
 *  not "eval_apply.turn_end" thirty times. This rollup counts the meaningful
 *  units; the raw pills stay reachable behind an expand toggle (map principle). */

export interface PatternRollup {
  /** Completed eval→apply cycles (one per turn_end). */
  readonly cycles: number;
  /** Detected sentences (turn.sentence). */
  readonly sentences: number;
  /** Any other pattern type we don't fold. */
  readonly other: number;
  /** Raw pattern count (what the expand toggle reveals). */
  readonly total: number;
}

/** A cycle completes at turn_end; eval/apply are its sub-steps (folded, not
 *  counted separately). Anything outside the eval_apply.* / turn.sentence
 *  families counts as `other`. */
export function summarizePatterns(patterns: readonly { type: string }[]): PatternRollup {
  let cycles = 0;
  let sentences = 0;
  let other = 0;
  for (const p of patterns) {
    if (p.type === "eval_apply.turn_end") cycles++;
    else if (p.type === "turn.sentence") sentences++;
    else if (p.type.startsWith("eval_apply.")) {
      // eval / apply sub-steps — folded into their cycle, not their own signal.
    } else other++;
  }
  return { cycles, sentences, other, total: patterns.length };
}

const plural = (n: number, one: string) => `${n} ${n === 1 ? one : one + "s"}`;

/** One calm line: "12 cycles · 8 sentences". Present parts only; empty string
 *  when there's nothing to say. */
export function patternRollupLabel(r: PatternRollup): string {
  const parts: string[] = [];
  if (r.cycles) parts.push(plural(r.cycles, "cycle"));
  if (r.sentences) parts.push(plural(r.sentences, "sentence"));
  if (r.other) parts.push(`${r.other} other`); // invariant — "others" reads wrong
  return parts.join(" · ");
}
