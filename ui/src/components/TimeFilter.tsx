/**
 * TimeFilter — pill-row scope filter for the Live sidebar.
 *
 * Sits next to `<PersonRow>` above the Sessions list. Click a pill to
 * narrow the visible session list by `latestTimestamp`. The "All" pill
 * clears the filter. Selected state inverts the pill (filled, not muted).
 *
 * URL-driven: the parent owns the filter state via `route.timeFilter`
 * and passes a setter that calls `navigate()` so the URL stays the
 * source of truth (bookmarkable / shareable).
 */

import { memo } from "react";
import {
  TIME_FILTER_LABELS,
  TIME_FILTER_ORDER,
  type TimeFilterKey,
} from "@/lib/time-filter";

export interface TimeFilterProps {
  /** Current selection. `null` is treated as "all" but rendered as no
   *  pill highlighted — caller can pass either; both work. */
  value: TimeFilterKey | null;
  onChange: (next: TimeFilterKey) => void;
}

export const TimeFilter = memo(function TimeFilter({ value, onChange }: TimeFilterProps) {
  const active: TimeFilterKey = value ?? "all";

  return (
    <div
      className="px-2 py-2 border-b border-[#2f3348] flex items-center gap-1.5 overflow-x-auto"
      data-testid="time-filter"
      role="group"
      aria-label="Filter sessions by time"
    >
      {TIME_FILTER_ORDER.map((key) => {
        const selected = active === key;
        return (
          <button
            key={key}
            type="button"
            onClick={() => onChange(key)}
            data-testid={`time-filter-${key}`}
            data-selected={selected}
            className={`shrink-0 px-3 py-1.5 rounded-lg text-[12px] font-medium transition-colors ${
              selected
                ? "bg-[#7aa2f7] text-[#1a1b26]"
                : "text-[#565f89] hover:text-[#c0caf5] hover:bg-[#24283b]"
            }`}
          >
            {TIME_FILTER_LABELS[key]}
          </button>
        );
      })}
    </div>
  );
});
