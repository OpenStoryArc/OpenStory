/** Facet filter groups for the merged Explore sidebar — extracted from the
 *  retired Overview so one component renders every facet family. */

import { useState } from "react";
import type { FacetValue } from "@/lib/sessions-overview";
import { cn } from "@/lib/cn";

export function FacetGroup({
  group,
  title,
  values,
  selected,
  onSelect,
  color,
}: {
  /** Stable lowercase id used in testids (facet-{group}-{key}). */
  group: string;
  title: string;
  values: FacetValue[];
  selected: string | undefined;
  onSelect: (key: string | undefined) => void;
  color?: (key: string) => string;
}) {
  const [showAll, setShowAll] = useState(false);
  if (values.length === 0) return null;
  const shown = showAll ? values : values.slice(0, 8);
  return (
    <div className="mb-3">
      <div className="mb-1 px-1 text-[10px] font-medium uppercase tracking-wide text-[color:var(--text-muted)]">{title}</div>
      <div className="flex flex-col gap-0.5">
        {shown.map((v) => {
          const active = selected === v.key;
          return (
            <button
              key={v.key}
              data-testid={`facet-${group}-${v.key}`}
              onClick={() => onSelect(active ? undefined : v.key)}
              className={cn(
                "flex items-center justify-between rounded px-2 py-0.5 text-left text-[11px] transition-colors",
                active ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text)] hover:bg-[color:var(--bg-hover)]",
              )}
            >
              <span className="flex items-center gap-1.5 truncate">
                {color && <span className="inline-block h-2 w-2 shrink-0 rounded-full" style={{ background: color(v.key) }} />}
                <span className="truncate">{v.key}</span>
              </span>
              <span className={cn("ml-2 shrink-0 tabular-nums", active ? "text-[color:var(--bg)]" : "text-[color:var(--text-muted)]")}>{v.count}</span>
            </button>
          );
        })}
        {values.length > 8 && (
          <button onClick={() => setShowAll((s) => !s)} className="px-2 py-0.5 text-left text-[10px] text-[color:var(--accent)] hover:text-[#89b4fa]">
            {showAll ? "Show less" : `+${values.length - 8} more`}
          </button>
        )}
      </div>
    </div>
  );
}
