/**
 * Pure functions for the Story view.
 *
 * Each function transforms sentence patterns into derived data
 * for rendering: category classification, filtering, grouping,
 * depth profiling, verb distribution, cross-linking.
 *
 * No side effects. No I/O. No React. Just data in, data out.
 */

import type { PatternView } from "@/types/wire-record";
import { extractDomainFact, extractDomainFacts, type DomainFact } from "@/lib/domain-facts";

// ═══════════════════════════════════════════════════════════════════
// Category classification
// ═══════════════════════════════════════════════════════════════════

export type StoryCategory =
  | "pure_text"    // No tools — just conversation
  | "tool_use"     // Tools dispatched
  | "thinking"     // Reasoning blocks present
  | "delegation"   // Agent tool used
  | "error";       // Tool errors occurred

/** Classify a turn.sentence pattern into a story category.
 *  Priority: error > delegation > thinking > tool_use > pure_text.
 */
export function categorizeTurn(pattern: PatternView): StoryCategory {
  const m = pattern.metadata ?? {};
  const applies = (m.applies as Array<{ is_error: boolean; is_agent: boolean }>) ?? [];
  const thinking = m.thinking as { summary: string } | null;
  const decision = ((m.eval as { decision?: string })?.decision) ?? "text_only";

  // Error takes priority
  if (applies.some(a => a.is_error)) return "error";

  // Delegation (Agent tool)
  if (applies.some(a => a.is_agent)) return "delegation";

  // Thinking present
  if (thinking?.summary) return "thinking";

  // Tool use
  if (applies.length > 0 || decision === "tool_use") return "tool_use";

  // Pure text
  return "pure_text";
}

/** Filter sentences by allowed categories.
 *  Empty filter set = show all (no filter applied).
 */
export function filterSentences(
  sentences: readonly PatternView[],
  allowedCategories: Set<StoryCategory>,
): PatternView[] {
  if (allowedCategories.size === 0) return [...sentences];
  return sentences.filter(s => allowedCategories.has(categorizeTurn(s)));
}

// ═══════════════════════════════════════════════════════════════════
// Scope depth profiling
// ═══════════════════════════════════════════════════════════════════

/** Extract scope_depth from each sentence as a sparkline series. */
export function scopeDepthProfile(sentences: readonly PatternView[]): number[] {
  return sentences.map(s => ((s.metadata ?? {}).scope_depth as number) ?? 0);
}

// ═══════════════════════════════════════════════════════════════════
// Session grouping
// ═══════════════════════════════════════════════════════════════════

/** Group sentences by session_id. */
export function groupBySession(
  sentences: readonly PatternView[],
): Map<string, PatternView[]> {
  const map = new Map<string, PatternView[]>();
  for (const s of sentences) {
    const arr = map.get(s.session_id) ?? [];
    arr.push(s);
    map.set(s.session_id, arr);
  }
  return map;
}

// ═══════════════════════════════════════════════════════════════════
// Verb distribution
// ═══════════════════════════════════════════════════════════════════

/** Count occurrences of each verb across sentences. */
export function verbDistribution(
  sentences: readonly PatternView[],
): Map<string, number> {
  const counts = new Map<string, number>();
  for (const s of sentences) {
    const verb = ((s.metadata ?? {}).verb as string) ?? "?";
    counts.set(verb, (counts.get(verb) ?? 0) + 1);
  }
  return counts;
}

// ═══════════════════════════════════════════════════════════════════
// Cross-linking
// ═══════════════════════════════════════════════════════════════════

/** Get the event IDs that compose this turn (for linking to Live view). */
export function turnEventIds(pattern: PatternView): readonly string[] {
  return pattern.events;
}

/** The turn's SOURCE event — where "click all the way into the turn" lands. The
 *  culminating (last) event is the turn's payoff; null when the turn has no
 *  events. The map principle: a Story turn is never a dead end. */
export function turnDrillTarget(pattern: PatternView): string | null {
  const ev = pattern.events;
  return ev.length > 0 ? ev[ev.length - 1]! : null;
}

// ═══════════════════════════════════════════════════════════════════
// Turn-in-progress detection
// ═══════════════════════════════════════════════════════════════════

/** A completed sentence pattern is never "in progress" — it was
 *  emitted because the turn completed. In-progress detection
 *  requires tracking the accumulator state, which lives on the
 *  backend. Completed patterns are always resolved. */
export function isInProgress(_pattern: PatternView): boolean {
  return false;
}

// ═══════════════════════════════════════════════════════════════════
// Environment growth tracking
// ═══════════════════════════════════════════════════════════════════

/** Extract env_size from each sentence as a growth series. */
export function envGrowthSeries(sentences: readonly PatternView[]): number[] {
  return sentences.map(s => ((s.metadata ?? {}).env_size as number) ?? 0);
}

// ═══════════════════════════════════════════════════════════════════
// Domain fact extraction from turn patterns
// ═══════════════════════════════════════════════════════════════════

interface ApplyWithOutcome {
  tool_outcome?: { type: string; path?: string; command?: string; succeeded?: boolean; pattern?: string; source?: string; description?: string; reason?: string } | null;
}

/** Extract deduplicated, sorted domain facts from a turn.sentence pattern's applies. */
export function turnDomainFacts(pattern: PatternView): DomainFact[] {
  const applies = ((pattern.metadata ?? {}).applies as ApplyWithOutcome[]) ?? [];
  return extractDomainFacts(applies);
}

// ═══════════════════════════════════════════════════════════════════
// Event map: pair applies with their domain facts
// ═══════════════════════════════════════════════════════════════════

export interface ApplyEventEntry {
  tool_name: string;
  fact: DomainFact | null;
}

// ═══════════════════════════════════════════════════════════════════
// Subagent turn resolution
// ═══════════════════════════════════════════════════════════════════

/** Get turn.sentence patterns for a subagent session, sorted by turn number. */
export function agentSessionTurns(
  agentSessionId: string,
  patterns: readonly PatternView[],
): PatternView[] {
  return patterns
    .filter(p => p.type === "turn.sentence" && p.session_id === agentSessionId)
    .sort((a, b) => {
      const ta = ((a.metadata ?? {}).turn as number) ?? 0;
      const tb = ((b.metadata ?? {}).turn as number) ?? 0;
      return ta - tb;
    });
}

/** Map each apply in a turn to its domain fact (or null if no outcome). */
export function turnEventMap(pattern: PatternView): ApplyEventEntry[] {
  const applies = ((pattern.metadata ?? {}).applies as Array<{
    tool_name: string;
    tool_outcome?: { type: string; path?: string; command?: string; succeeded?: boolean; pattern?: string; source?: string; description?: string; reason?: string } | null;
  }>) ?? [];

  return applies.map(a => ({
    tool_name: a.tool_name,
    fact: a.tool_outcome ? extractDomainFact(a.tool_outcome) : null,
  }));
}

/** Index of the sentence/turn whose events include `eventId` (for deep-linking
 *  Story to a specific event — `#/story/SES/event/ID`). -1 when not found or no
 *  eventId. Pure → the scroll-to-turn effect is testable. */
export function findSentenceIndexByEvent(
  sentences: readonly PatternView[],
  eventId: string | undefined,
): number {
  if (!eventId) return -1;
  return sentences.findIndex((s) => (s.events ?? []).includes(eventId));
}

/** Index of the sentence whose turn number is `turn` — the spine's click
 *  target in the feed (both are the same session's sentences; the feed is
 *  sorted by turn). -1 when missing. Pure → the click mapping is testable. */
export function findSentenceIndexByTurn(
  sentences: readonly PatternView[],
  turn: number | undefined,
): number {
  if (turn == null) return -1;
  return sentences.findIndex((s) => (s.metadata?.turn as number | undefined) === turn);
}

/** A short human headline for a turn.sentence — the verb + object of what the
 *  agent did that turn (e.g. "edited 5 source files"). Falls back to the
 *  pattern label. The verbatim rationale (the "because …" quote) is returned
 *  separately so the sidebar can lead with the action and whisper the why. */
export function sentenceHeadline(p: PatternView): { text: string; because: string | null } {
  const m = p.metadata ?? {};
  const verb = typeof m.verb === "string" ? m.verb.trim() : "";
  const object = typeof m.object === "string" ? m.object.trim() : "";
  const text = [verb, object].filter(Boolean).join(" ").trim() || p.label || "…";
  let because: string | null = null;
  if (typeof m.adverbial === "string" && m.adverbial.trim()) {
    const raw = m.adverbial.trim().replace(/^"|"$/g, "");
    because = raw.split("\n")[0]!.slice(0, 90).trim() || null;
  }
  return { text, because };
}
