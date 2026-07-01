/** Pure, deterministic layout for the Sessions Canvas — a Figma-like board of
 *  session activity clustered by project.
 *
 *  Inspired by the religious-freedom concept graph (bounded-context clusters
 *  that collapse/expand) but fixing its two rendering smells: the layout here is
 *  DETERMINISTIC (phyllotaxis, no Math.random → reproducible + testable) and the
 *  positions are computed once (the view renders persistent nodes, not a
 *  re-innerHTML'd string per frame). */

import type { StorySession } from "@/lib/story-api";
import { projectKey } from "@/lib/sessions-overview";

export interface CanvasCluster {
  readonly project: string;
  readonly x: number;
  readonly y: number;
  readonly count: number;
  readonly collapsed: boolean;
}

export interface CanvasNode {
  readonly id: string;
  readonly project: string;
  readonly x: number;
  readonly y: number;
  readonly events: number;
  readonly status: string;
  readonly label: string;
}

export interface CanvasModel {
  readonly clusters: CanvasCluster[];
  readonly nodes: CanvasNode[];
  readonly bounds: { minX: number; minY: number; maxX: number; maxY: number };
}

const GOLDEN = Math.PI * (3 - Math.sqrt(5)); // ~137.5° — even, gap-free packing
const CLUSTER_SPACING = 150;
const NODE_SPACING = 34;

/** i-th phyllotaxis point around an origin: even, deterministic sunflower spread. */
function phyllo(i: number, spacing: number): { x: number; y: number } {
  const r = spacing * Math.sqrt(i + 0.5);
  const a = i * GOLDEN;
  return { x: r * Math.cos(a), y: r * Math.sin(a) };
}

export function buildCanvas(
  sessions: readonly StorySession[],
  expandedProjects: ReadonlySet<string>,
): CanvasModel {
  // group by project
  const byProject = new Map<string, StorySession[]>();
  for (const s of sessions) {
    const p = projectKey(s);
    (byProject.get(p) ?? byProject.set(p, []).get(p)!).push(s);
  }

  // biggest projects near the center; ties broken by name for determinism
  const projects = [...byProject.entries()].sort(
    (a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]),
  );

  const clusters: CanvasCluster[] = [];
  const nodes: CanvasNode[] = [];
  let minX = 0, minY = 0, maxX = 0, maxY = 0;
  const track = (x: number, y: number) => {
    minX = Math.min(minX, x); minY = Math.min(minY, y);
    maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
  };

  projects.forEach(([project, list], i) => {
    const center = phyllo(i, CLUSTER_SPACING);
    const collapsed = !expandedProjects.has(project);
    clusters.push({ project, x: center.x, y: center.y, count: list.length, collapsed });
    track(center.x, center.y);

    if (collapsed) return;
    // bloom: pack this project's sessions around its center (deterministic order)
    const ordered = [...list].sort((a, b) => a.session_id.localeCompare(b.session_id));
    ordered.forEach((s, j) => {
      const p = phyllo(j, NODE_SPACING);
      const x = center.x + p.x;
      const y = center.y + p.y;
      nodes.push({
        id: s.session_id,
        project,
        x,
        y,
        events: s.event_count ?? 0,
        status: s.status ?? "completed",
        label: s.label || s.session_id.slice(0, 8),
      });
      track(x, y);
    });
  });

  return { clusters, nodes, bounds: { minX, minY, maxX, maxY } };
}
