/** The Live-tab filter pills, reusable. A row of facet chips with counts + an
 *  active state; disabled (greyed, non-clickable) when a facet has no matches.
 *  Presentational — the parent owns the active facet + supplies counts. */

import { cn } from "@/lib/cn";

export function FilterPills<F extends string>({
  facets,
  active,
  counts,
  onSelect,
}: {
  facets: readonly F[];
  active: F;
  counts: Record<F, number>;
  onSelect: (facet: F) => void;
}) {
  return (
    <div
      className="flex flex-wrap items-center gap-1 border-b border-[#2f3348] bg-[#1a1b26] px-3 py-1.5 text-[11px]"
      data-testid="conversation-filter-pills"
    >
      {facets.map((f) => {
        const count = counts[f] ?? 0;
        const label = f.charAt(0).toUpperCase() + f.slice(1);
        const empty = f !== ("all" as F) && count === 0;
        return (
          <button
            key={f}
            onClick={() => !empty && onSelect(f)}
            disabled={empty}
            data-testid={`conv-filter-${f}`}
            className={cn(
              "rounded px-2 py-0.5 transition-colors",
              active === f
                ? "bg-[#7aa2f7] font-medium text-[#1a1b26]"
                : empty
                  ? "cursor-default text-[#3b4261]"
                  : "text-[#565f89] hover:text-[#c0caf5]",
            )}
          >
            {label}
            {f !== ("all" as F) && count > 0 && <span className="ml-1 opacity-60">{count}</span>}
          </button>
        );
      })}
    </div>
  );
}
