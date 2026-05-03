/**
 * PersonChip — single user pill rendered inside `PersonRow`.
 *
 * Visual model from openstory-ui-prototype's `App.tsx:721-752`: round colored
 * avatar + bold name + active-pulse dot + session count subtitle. Adapted
 * to the OpenStory sidebar's tighter horizontal layout — chips wrap rather
 * than stack vertically.
 *
 * Color comes from `personColor()` so the same name → same hue across every
 * component (chip, badge on session card, future Story tab). The selected
 * state inverts the chip; the active-now state pulses a green dot.
 */

import { memo } from "react";
import { personColor } from "@/lib/person-color";

export interface PersonChipProps {
  /** User identifier — used as the displayed name and for color derivation. */
  user: string;
  /** Total session count for this user. Renders as a subtitle. */
  sessionCount: number;
  /** When true, the chip is highlighted as the current filter. */
  selected: boolean;
  /** When true, render a small pulsing green dot — "currently active". */
  isActiveNow: boolean;
  /** Click handler: typically toggles the user filter on/off. */
  onClick: () => void;
}

export const PersonChip = memo(function PersonChip({
  user,
  sessionCount,
  selected,
  isActiveNow,
  onClick,
}: PersonChipProps) {
  const color = personColor(user);
  const initial = user.slice(0, 2).toUpperCase();

  // Selected: bg-tinted with a 1px ring; unselected: muted hover state.
  const baseClasses =
    "flex items-center gap-2 px-2.5 py-1.5 rounded-lg transition-colors text-left shrink-0";
  const stateClasses = selected
    ? "ring-1"
    : "hover:bg-[#24283b]";

  return (
    <button
      type="button"
      onClick={onClick}
      data-testid={`person-chip-${user}`}
      data-selected={selected}
      className={`${baseClasses} ${stateClasses}`}
      style={
        selected
          ? {
              backgroundColor: `${color}25`,
              borderColor: color,
              boxShadow: `inset 0 0 0 1px ${color}`,
            }
          : undefined
      }
      title={selected ? `Clear ${user} filter` : `Filter to ${user}`}
    >
      <span
        className="w-7 h-7 rounded-full shrink-0 flex items-center justify-center text-[10px] font-bold relative"
        style={{ backgroundColor: `${color}30`, color }}
      >
        {initial}
        {isActiveNow && (
          <span
            className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-[#9ece6a] pulse-live"
            title="Currently active"
            aria-label="active now"
          />
        )}
      </span>
      <span className="flex flex-col items-start min-w-0">
        <span className="text-[12px] font-medium text-[#c0caf5] truncate max-w-[120px]">
          @{user}
        </span>
        <span className="text-[10px] text-[#565f89]">
          {sessionCount} session{sessionCount === 1 ? "" : "s"}
        </span>
      </span>
    </button>
  );
});
