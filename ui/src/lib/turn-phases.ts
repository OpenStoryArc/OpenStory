/**
 * Extract turn phase segments from PatternView[] for the TurnPhaseBar.
 *
 * Filters to turn.phase patterns, parses the phase name from the label,
 * and returns segments with proportional widths.
 */
import type { PatternView } from "@/types/wire-record";

export interface TurnPhaseSegment {
  readonly phase: string;
  readonly eventCount: number;
  readonly events: readonly string[];
}

/**
 * Extract turn phase segments from patterns.
 * Label format: "conversation (3 events)" or just "conversation".
 */
export function extractTurnPhases(patterns: readonly PatternView[]): TurnPhaseSegment[] {
  return patterns
    .filter((p) => p.type === "turn.phase")
    .map((p) => {
      // Parse phase name: "conversation (3 events)" → "conversation"
      const match = p.label.match(/^(.+?)(?:\s*\(|$)/);
      const phase = match ? match[1]!.trim() : p.label;
      return {
        phase,
        eventCount: p.events.length,
        events: p.events,
      };
    });
}
