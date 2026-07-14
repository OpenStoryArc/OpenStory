/** Story feed mode — the render decision for the Story view.
 *
 * Story renders `turn.sentence` patterns, which are derived by the patterns
 * consumer as events flow through the live local pipeline. Sessions whose events
 * arrived without the detectors running (federated backfill, older imports) have
 * records but ZERO patterns, so Story was blank for them. When there are no
 * sentences, we fall back to STRUCTURAL turns — eval→apply cycles extracted
 * client-side from the session's records (`extractCycles`). Degraded (no verb/
 * adverbial grammar) but never blank, and still read-only.
 *
 * Pure decision function → unit-tested; the component only renders the result.
 */

import type { EvalApplyCycle } from "./eval-apply";

export type StoryFeed =
  | { kind: "loading" }
  | { kind: "sentences" }
  | { kind: "structural"; cycles: EvalApplyCycle[] }
  | { kind: "empty"; selected: boolean };

export interface StoryFeedArgs {
  selectedSession: string | null;
  /** True while the `turn.sentence` patterns fetch is in flight. */
  loadingSentences: boolean;
  /** Number of sentence patterns available for the selected session. */
  sentenceCount: number;
  /** True while the records→cycles fallback fetch is in flight. */
  loadingFallback: boolean;
  /** Cycles from the records fallback: `null` = not yet fetched/attempted. */
  fallbackCycles: EvalApplyCycle[] | null;
}

/** Decide what the Story feed shows. Sentences win; the structural fallback only
 *  appears once we've confirmed the session has no sentences. */
export function storyFeed(args: StoryFeedArgs): StoryFeed {
  const { selectedSession, loadingSentences, sentenceCount, loadingFallback, fallbackCycles } = args;
  if (loadingSentences) return { kind: "loading" };
  if (sentenceCount > 0) return { kind: "sentences" };
  if (loadingFallback) return { kind: "loading" };
  if (fallbackCycles && fallbackCycles.length > 0) return { kind: "structural", cycles: fallbackCycles };
  return { kind: "empty", selected: selectedSession !== null };
}
