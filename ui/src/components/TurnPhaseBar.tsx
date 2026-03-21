/**
 * TurnPhaseBar — horizontal colored segments showing session turn phases.
 *
 * Each segment is proportional to the event count of that turn phase.
 * Colors follow the Tokyonight palette to match the rest of the UI.
 */
import { memo } from "react";
import type { TurnPhaseSegment } from "@/lib/turn-phases";

const PHASE_COLORS: Record<string, string> = {
  conversation: "#7aa2f7",              // blue
  exploration: "#2ac3de",               // cyan
  implementation: "#9ece6a",            // green
  "implementation+testing": "#e0af68",  // yellow
  testing: "#ff9e64",                   // orange
  execution: "#f7768e",                 // red
  delegation: "#bb9af7",                // purple
  mixed: "#565f89",                     // grey
};

function phaseColor(phase: string): string {
  return PHASE_COLORS[phase] ?? "#565f89";
}

interface Props {
  readonly segments: readonly TurnPhaseSegment[];
}

export const TurnPhaseBar = memo(function TurnPhaseBar({ segments }: Props) {
  if (segments.length === 0) return null;

  const total = segments.reduce((sum, s) => sum + s.eventCount, 0);
  if (total === 0) return null;

  return (
    <div
      className="flex h-1.5 bg-[#1a1b26] border-b border-[#2f3348]"
      data-testid="turn-phase-bar"
      title={segments.map(s => `${s.phase}: ${s.eventCount}`).join(" · ")}
    >
      {segments.map((seg, i) => {
        const width = (seg.eventCount / total) * 100;
        const color = phaseColor(seg.phase);
        return (
          <div
            key={`${seg.phase}-${i}`}
            className="h-full transition-all"
            style={{
              width: `${width}%`,
              backgroundColor: color,
              opacity: 0.7,
            }}
            title={`${seg.phase}: ${seg.eventCount} events`}
          />
        );
      })}
    </div>
  );
});
