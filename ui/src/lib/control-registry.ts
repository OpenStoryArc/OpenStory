/** Registry of component-local control targets the agent-in-UI seam can drive
 *  via `toggle` / `set`. Route-owned state (detailView, filters, sessionId, …)
 *  lives on HashRoute and is driven by open_view / query / focus_event — not
 *  listed here. This module is the vocabulary for *non-URL* knobs so agents
 *  and error messages can discover valid targets without grepping components.
 *
 *  "Drive the mirror, never the watched" — these only reshape what the UI
 *  shows. */

export interface ControlTargetDef {
  /** Short description of what the control does. */
  readonly description: string;
  /** Known accepted values when the control is a closed enum; omit for free-form. */
  readonly values?: readonly string[];
  /** Which action shapes apply: toggle (string value) and/or set (object). */
  readonly actions: readonly ("toggle" | "set")[];
}

/** Registered toggle/set targets. Keep in sync with component sinks that
 *  subscribe to controlActions$(). */
export const CONTROL_TARGETS: Readonly<Record<string, ControlTargetDef>> = {
  "canvas.mode": {
    description: "Sessions Canvas visualization mode",
    // Keep in lockstep with CANVAS_MODES in canvas-modes.ts.
    values: [
      "sunburst",
      "board",
      "treemap",
      "gantt",
      "scatter",
      "flow",
      "tool-adjacency",
      "agent-project",
      "durations",
      "heatmap",
    ],
    actions: ["toggle"],
  },
  "canvas.groupBy": {
    description: "Canvas grouping dimension",
    actions: ["toggle"],
  },
  "canvas.metric": {
    description: "Canvas size/color metric",
    values: ["events", "tokens"],
    actions: ["toggle"],
  },
  "canvas.drill": {
    description: "Drill into a canvas node (space-filling modes)",
    actions: ["toggle"],
  },
  "canvas.ascend": {
    description: "Ascend one level in canvas drill path",
    actions: ["toggle"],
  },
  "heatmap.dim": {
    description: "Heatmap 2D vs 3D",
    values: ["2d", "3d"],
    actions: ["toggle"],
  },
  "heatmap.weeks": {
    description: "How many weeks the heatmap shows",
    actions: ["toggle"],
  },
  "story.sort": {
    description: "Story session list sort",
    values: ["latest", "active", "tokens"],
    actions: ["toggle"],
  },
  "ask.question": {
    description: "Select an Ask view question id",
    actions: ["toggle"],
  },
  "session.lens": {
    description: "Session detail lens",
    values: ["conversation", "trace", "subagents", "details"],
    actions: ["toggle"],
  },
  "ribbon.compact": {
    description: "Activity ribbon compact density",
    values: ["on", "off"],
    actions: ["toggle"],
  },
  "ribbon.collapsed": {
    description: "Collapse the activity ribbon",
    values: ["on", "off"],
    actions: ["toggle"],
  },
  "tokens.collapsed": {
    description: "Collapse the token report panel",
    values: ["on", "off"],
    actions: ["toggle"],
  },
  theme: {
    description: "Dashboard color theme",
    values: ["light", "dark"],
    actions: ["toggle"],
  },
  spotlight: {
    description: "Dismiss presentation spotlight/title (value: off)",
    values: ["off"],
    actions: ["toggle"],
  },
  "scatter.brush": {
    description: "Scatter plot brush box (ev0/ev1/tok0/tok1)",
    actions: ["set"],
  },
};

/** All registered target keys (stable alpha order for docs/schema). */
export function knownControlTargets(): readonly string[] {
  return Object.keys(CONTROL_TARGETS).sort();
}

/** Whether `target` is a known registered control. Unknown targets still
 *  flow through interpretControl (open vocabulary) — views ignore what they
 *  don't own — but agents can validate before driving. */
export function isKnownControlTarget(target: string): boolean {
  return Object.prototype.hasOwnProperty.call(CONTROL_TARGETS, target);
}

/** Human-readable hint when an agent uses an unknown target. */
export function unknownTargetHint(target: string): string {
  const known = knownControlTargets().join(", ");
  return `Unknown control target '${target}'. Registered: ${known}. Route-owned state (detailView, filters, filePath, …) uses open_view / query / focus_event, not toggle.`;
}
