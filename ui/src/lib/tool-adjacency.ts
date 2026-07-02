/** Pure model for the Tool Transition Adjacency Heatmap (Lab candidate
 *  `tool-adjacency-heatmap`). A from×to grid of tool-transition frequencies —
 *  where Flow shows the ribbons, this shows the dense matrix so hot pairs read
 *  at a glance. Reuses countTransitions; side-effect-free so it's unit-tested. */

import { countTransitions } from "@/lib/tool-flow";

const SEP = "|"; // matches tool-flow's key separator

export interface AdjCell {
  readonly from: string;
  readonly to: string;
  readonly count: number;
}

export interface AdjMatrix {
  /** tools kept (top-N by transition volume), stable order for both axes. */
  readonly tools: string[];
  readonly cells: AdjCell[];
  readonly max: number;
  readonly total: number;
}

/** Build the from×to adjacency matrix over the top-N tools by transition volume. */
export function buildAdjacencyMatrix(
  sequences: readonly (readonly string[])[],
  { topN = 10 }: { topN?: number } = {},
): AdjMatrix {
  const trans = countTransitions(sequences); // Map<"from|to", count>

  // volume per tool (appears as from or to) → keep top-N
  const vol = new Map<string, number>();
  let total = 0;
  for (const [k, c] of trans) {
    const [a, b] = k.split(SEP) as [string, string];
    vol.set(a, (vol.get(a) ?? 0) + c);
    vol.set(b, (vol.get(b) ?? 0) + c);
    total += c;
  }
  const tools = [...vol.entries()]
    .sort((x, y) => y[1] - x[1] || x[0].localeCompare(y[0]))
    .slice(0, topN)
    .map((e) => e[0]);
  const keep = new Set(tools);

  const cells: AdjCell[] = [];
  let max = 0;
  for (const [k, count] of trans) {
    const [from, to] = k.split(SEP) as [string, string];
    if (!keep.has(from) || !keep.has(to)) continue;
    cells.push({ from, to, count });
    if (count > max) max = count;
  }
  cells.sort((a, b) => b.count - a.count || a.from.localeCompare(b.from) || a.to.localeCompare(b.to));
  return { tools, cells, max, total };
}
