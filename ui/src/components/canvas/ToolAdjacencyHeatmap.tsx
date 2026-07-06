/** ToolAdjacencyHeatmap — Lab candidate `tool-adjacency-heatmap`. A from×to grid
 *  of tool-transition frequencies: where Flow draws ribbons, this shows the dense
 *  matrix so hot pairs (Bash→Bash, Read→Edit) pop as bright cells. Samples recent
 *  claude-code sessions' records (like ToolFlowView) → sequences → the pure
 *  buildAdjacencyMatrix. Rendered inside the Lab. */

import { useEffect, useMemo, useState } from "react";
import type { StorySession } from "@/lib/story-api";
import type { WireRecord } from "@/types/wire-record";
import type { ToolCall } from "@/types/view-record";
import { buildAdjacencyMatrix } from "@/lib/tool-adjacency";
import { toolColor } from "@/lib/tool-colors";

const CELL = 40, LEFT = 120, TOP = 108, SAMPLE = 16;

export function ToolAdjacencyHeatmap({ sessions }: { sessions: readonly StorySession[] }) {
  const [sequences, setSequences] = useState<string[][]>([]);
  const [loading, setLoading] = useState(true);

  const sample = useMemo(
    () => sessions
      .filter((s) => (s.origin_agent || "") === "claude-code")
      .slice()
      .sort((a, b) => (b.last_event ? Date.parse(b.last_event) : 0) - (a.last_event ? Date.parse(a.last_event) : 0))
      .slice(0, SAMPLE),
    [sessions],
  );

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    const fetchRecs = async (id: string): Promise<WireRecord[] | null> => {
      const ctrl = new AbortController();
      const timer = setTimeout(() => ctrl.abort(), 8000);
      try {
        const r = await fetch(`/api/sessions/${id}/records`, { signal: ctrl.signal });
        const j = await r.json();
        return Array.isArray(j) ? (j as WireRecord[]) : [];
      } catch { return null; } finally { clearTimeout(timer); }
    };
    Promise.all(sample.map((s) => fetchRecs(s.session_id))).then((all) => {
      if (cancelled) return;
      const seqs = all
        .map((recs) => (recs ?? []).filter((r) => r.record_type === "tool_call").map((r) => (r.payload as ToolCall)?.name || "?"))
        .filter((s) => s.length > 1);
      setSequences(seqs);
      setLoading(false);
    });
    return () => { cancelled = true; };
  }, [sample]);

  const m = useMemo(() => buildAdjacencyMatrix(sequences, { topN: 10 }), [sequences]);
  const cellOf = (from: string, to: string) => m.cells.find((c) => c.from === from && c.to === to)?.count ?? 0;

  if (loading) return <div className="p-6 text-[12px] text-[#565f89]">Sampling recent sessions…</div>;
  if (m.tools.length === 0) return <div className="p-6 text-[12px] text-[#565f89]">No tool transitions found.</div>;

  const n = m.tools.length;
  const w = LEFT + n * CELL + 16;
  const h = TOP + n * CELL + 16;

  return (
    <div className="overflow-auto p-3">
      <div className="mb-2 text-[11px] text-[#565f89]">rows = from · columns = what came next · brighter = more frequent · {m.total} transitions</div>
      <svg width={w} height={h} className="block">
        <text x={LEFT + (n * CELL) / 2} y={16} textAnchor="middle" fontSize={10} fill="#565f89">→ to</text>
        {/* column (to) labels, rotated */}
        {m.tools.map((t, j) => (
          <text key={`c${t}`} transform={`translate(${LEFT + j * CELL + CELL / 2}, ${TOP - 6}) rotate(-45)`} fontSize={10} fill="#a9b1d6">{t}</text>
        ))}
        {/* row (from) labels */}
        {m.tools.map((t, i) => (
          <g key={`r${t}`}>
            <rect x={LEFT - 10} y={TOP + i * CELL + CELL / 2 - 4} width={6} height={6} rx={1} fill={toolColor(t)} />
            <text x={LEFT - 14} y={TOP + i * CELL + CELL / 2 + 4} textAnchor="end" fontSize={10} fill="#a9b1d6">{t}</text>
          </g>
        ))}
        {/* cells */}
        {m.tools.map((from, i) =>
          m.tools.map((to, j) => {
            const c = cellOf(from, to);
            const intensity = m.max > 0 ? Math.sqrt(c / m.max) : 0;
            return (
              <g key={`${i}-${j}`}>
                <rect
                  x={LEFT + j * CELL} y={TOP + i * CELL} width={CELL - 2} height={CELL - 2} rx={3}
                  fill={c > 0 ? "#7aa2f7" : "#1c2030"} fillOpacity={c > 0 ? 0.12 + 0.85 * intensity : 1}
                  stroke={i === j ? "#3b4261" : "none"} strokeWidth={i === j ? 1 : 0}
                >
                  <title>{`${from} → ${to} · ${c}`}</title>
                </rect>
                {c > 0 && (
                  <text x={LEFT + j * CELL + (CELL - 2) / 2} y={TOP + i * CELL + (CELL - 2) / 2 + 3} textAnchor="middle" fontSize={9} fill={intensity > 0.5 ? "#16171f" : "#a9b1d6"}>{c}</text>
                )}
              </g>
            );
          }),
        )}
      </svg>
    </div>
  );
}
