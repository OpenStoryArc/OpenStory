/** Shape panel — the session's deterministic shape footprint (bash / path / change). */

import { useState, useEffect } from "react";
import type { ShapeRow, RankedItem } from "@/lib/shape-detail";
import { rankField, sumChangeField, countByType } from "@/lib/shape-detail";

interface ShapesPanelProps {
  sessionId: string;
}

export function ShapesPanel({ sessionId }: ShapesPanelProps) {
  const [shapes, setShapes] = useState<readonly ShapeRow[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    setShapes(null);
    fetch(`/api/sessions/${sessionId}/shapes`)
      .then((r) => r.json())
      .then((res) => res?.shapes ?? [])
      .catch(() => [])
      .then((rows: ShapeRow[]) => {
        if (!cancelled) setShapes(rows);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  if (shapes === null) {
    return <div className="text-xs text-[#565f89]">Loading shapes…</div>;
  }
  if (shapes.length === 0) {
    return (
      <div className="text-xs text-[#565f89]">
        No shapes yet — run <span className="font-mono">open-story backfill-shapes</span> or wait for new activity.
      </div>
    );
  }

  const programs = rankField(shapes, "bash-shape", "program");
  const subcommands = rankField(shapes, "bash-shape", "subcommand");
  const segments = rankField(shapes, "path-shape", "top_segment");
  const tokens = rankField(shapes, "path-shape", "naming_tokens");
  const linesAdded = sumChangeField(shapes, "lines_added");
  const linesRemoved = sumChangeField(shapes, "lines_removed");

  return (
    <div className="rounded border border-[#2f3348] bg-[#24283b] p-3" data-testid="shapes-panel">
      <div className="mb-2 flex items-center justify-between">
        <div className="text-[11px] font-medium text-[#c0caf5]">Shape</div>
        <div className="text-[10px] text-[#565f89]">
          {countByType(shapes, "bash-shape")} bash · {countByType(shapes, "path-shape")} path ·{" "}
          {countByType(shapes, "change-shape")} change
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <RankedList title="Shell programs" items={programs} accent="#7aa2f7" />
        <RankedList title="Subcommands" items={subcommands} accent="#7aa2f7" />
        <RankedList title="Top segments" items={segments} accent="#9ece6a" />
        <RankedList title="Naming vocab" items={tokens} accent="#9ece6a" />
      </div>

      <div className="mt-3 flex gap-4 text-[11px]">
        <span className="text-[#565f89]">change delta</span>
        <span className="text-[#9ece6a]">+{linesAdded}</span>
        <span className="text-[#f7768e]">−{linesRemoved}</span>
      </div>
    </div>
  );
}

function RankedList({
  title,
  items,
  accent,
}: {
  title: string;
  items: readonly RankedItem[];
  accent: string;
}) {
  return (
    <div>
      <div className="mb-1.5 text-[10px] text-[#565f89]">
        {title} ({items.length})
      </div>
      {items.length === 0 ? (
        <div className="text-[11px] text-[#565f89]">—</div>
      ) : (
        <div className="max-h-[150px] space-y-0.5 overflow-y-auto">
          {items.map((it) => (
            <div
              key={it.value}
              className="flex items-center gap-2 rounded px-1 py-0.5 text-xs hover:bg-[#1a1b26]"
            >
              <span className="min-w-0 flex-1 truncate font-mono text-[#a9b1d6]">{it.value}</span>
              <span className="shrink-0 text-[10px]" style={{ color: accent }}>
                {it.count}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
