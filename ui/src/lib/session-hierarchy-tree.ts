/** Pure builder for a space-filling hierarchy (sunburst / treemap) of sessions.
 *
 *  Produces a nested {name, children, value} tree — Group → Project → Session
 *  (Project → Session when grouping by project) — that d3.hierarchy().sum() can
 *  lay out as a radial partition (sunburst) or squarified treemap. Leaves are
 *  sessions valued by a magnitude metric (events or tokens); internal nodes get
 *  their sum from d3. Applies the heavy-tail rule the explorers converged on:
 *  cap children at top-N by magnitude and roll the rest into an "other (K)"
 *  node, so the head stays readable and the long tail is still represented. */

import type { StorySession } from "@/lib/story-api";
import { projectKey } from "@/lib/sessions-overview";
import type { GroupDim } from "@/lib/sessions-canvas";

export type Metric = "events" | "tokens";

export interface TreeNode {
  readonly key: string;
  readonly name: string;
  readonly kind: "root" | "group" | "project" | "session" | "other";
  readonly sessionId?: string;
  /** leaf magnitude (raw); internal nodes leave this undefined and let d3 sum. */
  readonly value?: number;
  readonly children?: TreeNode[];
}

export function metricValue(s: StorySession, metric: Metric): number {
  if (metric === "events") return s.event_count ?? 0;
  // note: /api/sessions omits cache tokens, so this is input+output only
  return (s.total_input_tokens ?? 0) + (s.total_output_tokens ?? 0);
}

function dimValue(s: StorySession, dim: GroupDim): string {
  switch (dim) {
    case "user": return s.user || "unknown";
    case "agent": return s.origin_agent || "unknown";
    case "status": return s.status || "unknown";
    case "host": return s.host || "unknown";
    case "day": return (s.start_time ?? "").slice(0, 10) || "undated";
    case "project": return projectKey(s);
  }
}

function sumMetric(list: readonly StorySession[], metric: Metric): number {
  return list.reduce((a, s) => a + metricValue(s, metric), 0);
}

/** Keep the top-N children by magnitude; roll the remainder into one "other (K)"
 *  leaf valued by the summed remainder. Groups with <= N children are untouched. */
function capChildren(children: TreeNode[], topN: number, remainderValue: (dropped: TreeNode[]) => number): TreeNode[] {
  if (children.length <= topN) return children;
  const kept = children.slice(0, topN);
  const dropped = children.slice(topN);
  kept.push({ key: `other:${kept[0]?.key}`, name: `other (${dropped.length})`, kind: "other", value: remainderValue(dropped) });
  return kept;
}

export interface TreeOpts { topN?: number }

export function buildHierarchyTree(
  sessions: readonly StorySession[],
  groupBy: GroupDim,
  metric: Metric,
  opts: TreeOpts = {},
): TreeNode {
  const topN = opts.topN ?? 12;
  const byProject = groupBy === "project";

  const sessionLeaf = (s: StorySession): TreeNode => ({
    key: `s:${s.session_id}`, name: s.label || s.session_id.slice(0, 8), kind: "session",
    sessionId: s.session_id, value: metricValue(s, metric),
  });

  // group helper: map → sorted-by-magnitude entries
  const grouped = (list: readonly StorySession[], keyFn: (s: StorySession) => string) => {
    const m = new Map<string, StorySession[]>();
    for (const s of list) { const k = keyFn(s); (m.get(k) ?? m.set(k, []).get(k)!).push(s); }
    return [...m.entries()].sort((a, b) => sumMetric(b[1], metric) - sumMetric(a[1], metric) || a[0].localeCompare(b[0]));
  };

  const projectsOf = (list: readonly StorySession[], prefix: string): TreeNode[] => {
    const projs = grouped(list, projectKey).map(([proj, plist]): TreeNode => ({
      key: `${prefix}p:${proj}`, name: proj, kind: "project",
      children: plist.map(sessionLeaf),
    }));
    return capChildren(projs, topN, (dropped) =>
      dropped.reduce((a, n) => a + (n.children ?? []).reduce((b, c) => b + (c.value ?? 0), 0), 0));
  };

  let children: TreeNode[];
  if (byProject) {
    children = projectsOf(sessions, "");
  } else {
    const groups = grouped(sessions, (s) => dimValue(s, groupBy));
    // day groups read best latest-first
    if (groupBy === "day") groups.sort((a, b) => b[0].localeCompare(a[0]));
    children = capChildren(
      groups.map(([val, glist]): TreeNode => ({
        key: `g:${val}`, name: val, kind: "group", children: projectsOf(glist, `g:${val}:`),
      })),
      topN,
      (dropped) => dropped.reduce((a, n) => a + sumTree(n), 0),
    );
  }

  return { key: "root", name: "All sessions", kind: "root", children };
}

/** Sum leaf values under a node (for "other" remainders on internal nodes). */
function sumTree(n: TreeNode): number {
  if (n.value != null) return n.value;
  return (n.children ?? []).reduce((a, c) => a + sumTree(c), 0);
}
