/** Pure, deterministic layout for the Sessions Canvas — now a COLLAPSIBLE
 *  HIERARCHY, not a flat starfield (393 clusters at once was overwhelming).
 *
 *  Tree: Group → Project → Session (Project → Session when grouping by project).
 *  Only children of *expanded* nodes are materialized, so the canvas starts at a
 *  handful of top-level groups and deepens as you drill in. Nested phyllotaxis
 *  layout (golden-angle, no Math.random) keeps it deterministic + testable, and
 *  the parent→child edges make the tree legible. */

import type { StorySession } from "@/lib/story-api";
import { projectKey, sessionDayKey } from "@/lib/sessions-overview";

export type GroupDim = "day" | "user" | "agent" | "status" | "host" | "project";

export interface HNode {
  readonly key: string;
  readonly kind: "group" | "project" | "session";
  readonly label: string;
  readonly level: number;
  readonly parentKey: string | null;
  readonly x: number;
  readonly y: number;
  /** sessions beneath this node (1 for a session). */
  readonly count: number;
  readonly events: number;
  readonly status: string;
  readonly sessionId: string | null;
  /** true when it has children not currently shown. */
  readonly collapsed: boolean;
}

export interface HEdge { readonly from: string; readonly to: string; }

export interface Hierarchy {
  readonly nodes: HNode[];
  readonly edges: HEdge[];
  readonly bounds: { minX: number; minY: number; maxX: number; maxY: number };
}

const GOLDEN = Math.PI * (3 - Math.sqrt(5));
const SPACING = { group: 260, project: 96, session: 30 };

function phyllo(i: number, spacing: number): { x: number; y: number } {
  const r = spacing * Math.sqrt(i + 0.6);
  const a = i * GOLDEN;
  return { x: r * Math.cos(a), y: r * Math.sin(a) };
}

function dimValue(s: StorySession, dim: GroupDim): string {
  switch (dim) {
    case "day": return sessionDayKey(s) || "undated";
    case "user": return s.user || "unknown";
    case "agent": return s.origin_agent || "unknown";
    case "status": return s.status || "unknown";
    case "host": return s.host || "unknown";
    case "project": return projectKey(s);
  }
}

/** Group an array by a key, returning entries sorted by size desc then name. */
function groupSorted<T>(items: readonly T[], key: (t: T) => string): [string, T[]][] {
  const m = new Map<string, T[]>();
  for (const it of items) { const k = key(it); (m.get(k) ?? m.set(k, []).get(k)!).push(it); }
  return [...m.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]));
}

export function buildHierarchy(
  sessions: readonly StorySession[],
  groupBy: GroupDim,
  expanded: ReadonlySet<string>,
): Hierarchy {
  const nodes: HNode[] = [];
  const edges: HEdge[] = [];
  let minX = 0, minY = 0, maxX = 0, maxY = 0;
  const track = (x: number, y: number) => {
    minX = Math.min(minX, x); minY = Math.min(minY, y);
    maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
  };

  const byProject = groupBy === "project";

  // Level 0: top groups (or projects, when grouping by project)
  const topKey = (val: string) => (byProject ? `p:${val}` : `g:${val}`);
  const top = groupSorted(sessions, (s) => (byProject ? projectKey(s) : dimValue(s, groupBy)));
  // By day, order chronologically with the LATEST first ("what are the latest?")
  if (groupBy === "day") top.sort((a, b) => b[0].localeCompare(a[0]));

  top.forEach(([val, list], i) => {
    const c = phyllo(i, SPACING.group);
    const key = topKey(val);
    const kind = byProject ? "project" : "group";
    const groupExpanded = expanded.has(key);
    nodes.push({ key, kind, label: val, level: 0, parentKey: null, x: c.x, y: c.y, count: list.length, events: 0, status: "", sessionId: null, collapsed: !groupExpanded });
    track(c.x, c.y);
    if (!groupExpanded) return;

    if (byProject) {
      // project → sessions
      placeSessions(list, key, c.x, c.y);
    } else {
      // group → projects
      const projects = groupSorted(list, (s) => projectKey(s));
      projects.forEach(([proj, plist], j) => {
        const pc = phyllo(j, SPACING.project);
        const px = c.x + pc.x, py = c.y + pc.y;
        const pkey = `p:${val}:${proj}`;
        const projExpanded = expanded.has(pkey);
        nodes.push({ key: pkey, kind: "project", label: proj, level: 1, parentKey: key, x: px, y: py, count: plist.length, events: 0, status: "", sessionId: null, collapsed: !projExpanded });
        edges.push({ from: key, to: pkey });
        track(px, py);
        if (projExpanded) placeSessions(plist, pkey, px, py);
      });
    }
  });

  function placeSessions(list: readonly StorySession[], parentKey: string, cx: number, cy: number) {
    const ordered = [...list].sort((a, b) => a.session_id.localeCompare(b.session_id));
    ordered.forEach((s, k) => {
      const sc = phyllo(k, SPACING.session);
      const sx = cx + sc.x, sy = cy + sc.y;
      const skey = `s:${s.session_id}`;
      nodes.push({
        key: skey, kind: "session", label: s.label || s.session_id.slice(0, 8),
        level: byProject ? 1 : 2, parentKey, x: sx, y: sy, count: 1,
        events: s.event_count ?? 0, status: s.status ?? "completed", sessionId: s.session_id, collapsed: false,
      });
      edges.push({ from: parentKey, to: skey });
      track(sx, sy);
    });
  }

  return { nodes, edges, bounds: { minX, minY, maxX, maxY } };
}
