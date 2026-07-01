/** Pure fuzzy matching + ranking for the command palette.
 *
 *  A greedy subsequence matcher with the scoring heuristics every good palette
 *  shares: consecutive runs and word-boundary (acronym) hits rank above
 *  scattered matches, and earlier matches beat later ones. Side-effect-free so
 *  the ranking is unit-tested independently of the palette UI.
 */

export interface FuzzyResult {
  readonly score: number;
  readonly positions: number[];
}

function isBoundary(prevChar: string | undefined): boolean {
  if (prevChar === undefined) return true; // start of string
  return /[\s\-_/.:@]/.test(prevChar);
}

/**
 * Match `query` as a subsequence of `target` (case-insensitive). Returns null
 * if not a subsequence. Empty query matches with score 0.
 */
export function fuzzyMatch(query: string, target: string): FuzzyResult | null {
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  if (q.length === 0) return { score: 0, positions: [] };

  const positions: number[] = [];
  let score = 0;
  let ti = 0;
  let prevMatch = -2;

  for (let qi = 0; qi < q.length; qi++) {
    const ch = q[qi]!;
    let found = -1;
    for (; ti < t.length; ti++) {
      if (t[ti] === ch) {
        found = ti;
        break;
      }
    }
    if (found === -1) return null; // remaining query char not found

    // base
    score += 1;
    // consecutive run
    if (found === prevMatch + 1) score += 3;
    // word boundary (acronym-style)
    if (isBoundary(t[found - 1])) score += 5;
    // very first char of target
    if (found === 0) score += 3;

    positions.push(found);
    prevMatch = found;
    ti = found + 1;
  }

  // Prefer earlier overall matches (small tiebreaker).
  score -= (positions[0] ?? 0) * 0.1;
  return { score, positions };
}

/**
 * Rank items by fuzzy match over `getText(item)`; drops non-matches. An empty
 * query returns the items unchanged, capped to `limit`.
 */
export function rankItems<T>(query: string, items: readonly T[], getText: (item: T) => string, limit = 20): T[] {
  if (query.trim().length === 0) return items.slice(0, limit);

  const scored: { item: T; score: number; idx: number }[] = [];
  items.forEach((item, idx) => {
    const r = fuzzyMatch(query.trim(), getText(item));
    if (r) scored.push({ item, score: r.score, idx });
  });

  scored.sort((a, b) => b.score - a.score || a.idx - b.idx);
  return scored.slice(0, limit).map((s) => s.item);
}
