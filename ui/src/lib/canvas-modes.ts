/** Canvas view-mode metadata — the single source for the mode tabs' icon,
 *  label, one-line tooltip, and whether the group-by control applies. Keeping it
 *  here (not inline in the component) lets the completeness + group-by rules be
 *  unit-tested. */

export const CANVAS_MODES = ["board", "sunburst", "treemap", "gantt", "scatter", "flow", "tool-adjacency", "delegation", "agent-project"] as const;
export type CanvasMode = (typeof CANVAS_MODES)[number];

export interface ModeMeta {
  readonly icon: string;
  readonly label: string;
  /** short "what is this" tooltip for the tab. */
  readonly blurb: string;
  /** whether the group-by dimension chips apply to this mode. */
  readonly usesGroupBy: boolean;
  /** shown in place of the group-by row when usesGroupBy is false, so the
   *  control's absence is explained rather than mysterious. */
  readonly groupByNote?: string;
}

export const MODE_META: Record<CanvasMode, ModeMeta> = {
  board: { icon: "●", label: "Board", blurb: "Bubbles you expand: group → project → session.", usesGroupBy: true },
  sunburst: { icon: "◔", label: "Sunburst", blurb: "Radial rings sized by events or tokens; click to zoom.", usesGroupBy: true },
  treemap: { icon: "▦", label: "Treemap", blurb: "Nested rectangles sized by events or tokens.", usesGroupBy: true },
  gantt: { icon: "▭", label: "Gantt", blurb: "Sessions on a time axis, lane-packed by group.", usesGroupBy: true },
  scatter: { icon: "∴", label: "Scatter", blurb: "Each session a dot: events × output tokens.", usesGroupBy: false, groupByNote: "grouping n/a — every session is one point" },
  flow: { icon: "⇄", label: "Flow", blurb: "Tool→tool grammar for one agent.", usesGroupBy: false, groupByNote: "pick an agent in the view below" },
  "tool-adjacency": { icon: "▤", label: "Adjacency", blurb: "Which tool follows which, as a from×to heatmap across all sessions.", usesGroupBy: false, groupByNote: "aggregated across every session — no grouping" },
  delegation: { icon: "⇲", label: "Delegation", blurb: "The spawn topology: which parent sessions delegate to which agent-* subagents.", usesGroupBy: false, groupByNote: "spawn topology — parents and their subagents, not group-by" },
  "agent-project": { icon: "⊞", label: "Agents×Projects", blurb: "Which agent works in which project, as an event-weighted grid.", usesGroupBy: false, groupByNote: "the grid IS agent × project — grouping is built in" },
};

export function modeUsesGroupBy(m: CanvasMode): boolean {
  return MODE_META[m].usesGroupBy;
}
