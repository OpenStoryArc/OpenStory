/** Pure model for the tool->tool flow (the "grammar of agent work").
 *
 *  Counts ordered tool_call->tool_call transitions across a set of sessions and
 *  lays them out as a BIPARTITE sankey (from-tools on the left, to-tools on the
 *  right) - bipartite sidesteps the cycles (Bash<->Edit) and self-loops
 *  (Bash->Bash) that a classic left-to-right sankey can't handle, and needs no
 *  extra dep. Deterministic layout + counting -> unit-tested. */

export interface FlowNode {
  readonly tool: string;
  readonly side: "from" | "to";
  readonly value: number;
  readonly y0: number;
  readonly y1: number;
}

export interface FlowLink {
  readonly from: string;
  readonly to: string;
  readonly value: number;
  /** ribbon endpoints (y at the left node and right node). */
  readonly sy: number;
  readonly ty: number;
  readonly width: number;
}

export interface ToolFlow {
  readonly fromNodes: FlowNode[];
  readonly toNodes: FlowNode[];
  readonly links: FlowLink[];
  readonly total: number;
}

const SEP = "|"; // tool names never contain a pipe

/** Count consecutive tool->tool transitions across ordered per-session sequences. */
export function countTransitions(sequences: readonly (readonly string[])[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const seq of sequences) {
    for (let i = 0; i + 1 < seq.length; i++) {
      const k = seq[i] + SEP + seq[i + 1];
      m.set(k, (m.get(k) ?? 0) + 1);
    }
  }
  return m;
}

export interface FlowOpts { height: number; minCount?: number; topN?: number; gap?: number }

export function buildToolFlow(sequences: readonly (readonly string[])[], opts: FlowOpts): ToolFlow {
  const { height, minCount = 2, topN = 8, gap = 4 } = opts;
  const trans = countTransitions(sequences);

  // tool volume (appearances as from or to) -> keep top-N, fold rest into "other"
  const vol = new Map<string, number>();
  for (const [k, c] of trans) {
    const [a, b] = k.split(SEP) as [string, string];
    vol.set(a, (vol.get(a) ?? 0) + c);
    vol.set(b, (vol.get(b) ?? 0) + c);
  }
  const top = new Set([...vol.entries()].sort((x, y) => y[1] - x[1] || x[0].localeCompare(y[0])).slice(0, topN).map((e) => e[0]));
  const fold = (t: string) => (top.has(t) ? t : "other");

  // re-aggregate with folding, drop rare
  const agg = new Map<string, number>();
  for (const [k, c] of trans) {
    const [a, b] = k.split(SEP) as [string, string];
    const key = fold(a) + SEP + fold(b);
    agg.set(key, (agg.get(key) ?? 0) + c);
  }
  const links0 = [...agg.entries()]
    .map(([k, value]) => { const [from, to] = k.split(SEP) as [string, string]; return { from, to, value }; })
    .filter((l) => l.value >= minCount)
    .sort((a, b) => b.value - a.value || a.from.localeCompare(b.from) || a.to.localeCompare(b.to));

  const total = links0.reduce((a, l) => a + l.value, 0);
  if (total === 0) return { fromNodes: [], toNodes: [], links: [], total: 0 };

  const fromTotals = new Map<string, number>();
  const toTotals = new Map<string, number>();
  for (const l of links0) {
    fromTotals.set(l.from, (fromTotals.get(l.from) ?? 0) + l.value);
    toTotals.set(l.to, (toTotals.get(l.to) ?? 0) + l.value);
  }

  const stack = (totals: Map<string, number>, side: "from" | "to"): { nodes: FlowNode[]; cursor: Map<string, number> } => {
    const order = [...totals.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    const gaps = Math.max(order.length - 1, 0) * gap;
    const scale = (height - gaps) / total;
    const nodes: FlowNode[] = [];
    let y = 0;
    for (const [tool, value] of order) {
      const h = value * scale;
      nodes.push({ tool, side, value, y0: y, y1: y + h });
      y += h + gap;
    }
    return { nodes, cursor: new Map(nodes.map((n) => [n.tool, n.y0])) };
  };

  const fromStack = stack(fromTotals, "from");
  const toStack = stack(toTotals, "to");
  const scale = (height - Math.max(fromTotals.size - 1, 0) * gap) / total;

  const links: FlowLink[] = links0.map((l) => {
    const width = l.value * scale;
    const sy = fromStack.cursor.get(l.from)!;
    const ty = toStack.cursor.get(l.to)!;
    fromStack.cursor.set(l.from, sy + width);
    toStack.cursor.set(l.to, ty + width);
    return { ...l, sy: sy + width / 2, ty: ty + width / 2, width };
  });

  return { fromNodes: fromStack.nodes, toNodes: toStack.nodes, links, total };
}

/** What the pointer is hovering in the flow, driving path highlight. `null` =
 *  nothing hovered (everything shown at full strength). */
export type FlowHover =
  | { readonly type: "link"; readonly from: string; readonly to: string }
  | { readonly type: "from"; readonly tool: string }
  | { readonly type: "to"; readonly tool: string };

/** Is this ribbon part of the hovered path? Hovering a ribbon lights just that
 *  ribbon; hovering a from-node lights every ribbon leaving it; a to-node lights
 *  every ribbon entering it. Node highlight is derived from this in the view (a
 *  node is lit iff an active ribbon touches it on its side). Pure → tested. */
export function linkActive(l: { from: string; to: string }, h: FlowHover | null): boolean {
  if (!h) return true;
  if (h.type === "link") return l.from === h.from && l.to === h.to;
  if (h.type === "from") return l.from === h.tool;
  return l.to === h.tool;
}
