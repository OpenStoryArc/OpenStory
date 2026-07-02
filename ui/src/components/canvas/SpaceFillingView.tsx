/** SpaceFillingView — sunburst / treemap of the session hierarchy where
 *  AREA (or angle) = magnitude, the encoding the node-board Canvas can't do.
 *
 *  Same Group→Project→Session hierarchy + group-by as the board, valued by a
 *  metric (events or tokens, sqrt-compressed for the heavy tail). Click an
 *  internal node to zoom into it (breadcrumb to ascend); click a session leaf to
 *  open its details. d3-hierarchy partition/treemap are deterministic. */

import { useEffect, useMemo, useState } from "react";
import { hierarchy, partition, treemap, treemapSquarify, type HierarchyNode } from "d3-hierarchy";
import { arc as d3arc } from "d3-shape";
import type { StorySession } from "@/lib/story-api";
import type { GroupDim } from "@/lib/sessions-canvas";
import { buildHierarchyTree, type Metric, type TreeNode } from "@/lib/session-hierarchy-tree";
import { cleanHarnessPreview } from "@/lib/harness-message";
import { sunburstLabelLayout, sunburstCenterText } from "@/lib/sunburst-label";
import { controlActions$ } from "@/streams/control";

interface Props {
  sessions: readonly StorySession[];
  groupBy: GroupDim;
  metric: Metric;
  mode: "sunburst" | "treemap";
  width: number;
  height: number;
  onOpenSession: (sessionId: string) => void;
}

type HN = HierarchyNode<TreeNode>;

// Categorical palette (Tokyonight hues) — distinct top-level families, not a hash.
const PALETTE = ["#7aa2f7", "#bb9af7", "#9ece6a", "#e0af68", "#f7768e", "#2ac3de", "#ff9e64", "#7dcfff", "#73daca", "#c0caf5"];

/** Color descendants by which of the CURRENT root's top-level segments they
 *  belong to, so visible siblings are always maximally distinct (regardless of
 *  drill depth). "other" stays muted grey. */
function makeColor(laidRoot: HN): (n: HN) => string {
  const topIdx = new Map((laidRoot.children ?? []).map((c, i) => [c.data.key, i]));
  return (n: HN) => {
    if (n.data.kind === "other") return "#565f89";
    const top = n.ancestors().reverse()[1] ?? n; // depth-1 within the laid root
    return PALETTE[(topIdx.get(top.data.key) ?? 0) % PALETTE.length]!;
  };
}

export function SpaceFillingView({ sessions, groupBy, metric, mode, width, height, onOpenSession }: Props) {
  const tree = useMemo(() => buildHierarchyTree(sessions, groupBy, metric), [sessions, groupBy, metric]);
  // focus path (root → … → current); drilling pushes, breadcrumb pops
  const [focusKeys, setFocusKeys] = useState<string[]>([]);
  useEffect(() => { setFocusKeys([]); }, [tree]);

  const root = useMemo(
    () => hierarchy<TreeNode>(tree, (d) => d.children).sum((d) => (d.value != null ? Math.sqrt(d.value + 1) : 0)).sort((a, b) => (b.value ?? 0) - (a.value ?? 0)),
    [tree],
  );

  // resolve focus node by walking the key path
  const focus = useMemo(() => {
    let n: HN = root;
    for (const k of focusKeys) {
      const next = n.children?.find((c) => c.data.key === k);
      if (!next) break;
      n = next;
    }
    return n;
  }, [root, focusKeys]);

  const breadcrumb = focus.ancestors().reverse();

  const drill = (n: HN) => {
    if (n.data.kind === "session" && n.data.sessionId) { onOpenSession(n.data.sessionId); return; }
    if (n.children) setFocusKeys(n.ancestors().reverse().slice(1).map((a) => a.data.key));
  };

  // Agent-in-UI: drive drill-in (reshape the viz) via control$. `canvas.drill`
  // matches a wedge/cell by key or name (case-insensitive) and zooms into it —
  // the same effect as a user click, from outside. `canvas.ascend` pops up.
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type !== "toggle") return;
      if (a.target === "canvas.drill" && a.value) {
        const t = a.value.toLowerCase();
        const node = root.descendants().find(
          (n) => n.depth > 0 && n.data.kind !== "session" &&
            (n.data.key.toLowerCase() === t || (n.data.name ?? "").toLowerCase().includes(t)),
        );
        if (node) setFocusKeys(node.ancestors().reverse().slice(1).map((x) => x.data.key));
      } else if (a.target === "canvas.ascend") {
        setFocusKeys((prev) => prev.slice(0, -1));
      }
    });
    return () => sub.unsubscribe();
  }, [root]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* breadcrumb */}
      <div className="flex items-center gap-1 px-3 py-1.5 text-[11px] text-[#565f89]">
        {breadcrumb.map((a, i) => (
          <span key={a.data.key} className="flex items-center gap-1">
            {i > 0 && <span className="text-[#3b4261]">/</span>}
            <button onClick={() => setFocusKeys(a.ancestors().reverse().slice(1).map((x) => x.data.key))} className="hover:text-[#c0caf5]">
              {i === 0 ? "all" : cleanHarnessPreview(a.data.name).split(/[/]/).pop()?.slice(0, 20)}
            </button>
          </span>
        ))}
        <span className="ml-2 text-[#565f89]">· {metric}</span>
      </div>
      <div className="relative min-h-0 flex-1 bg-[#16171f]">
        {mode === "sunburst"
          ? <Sunburst focus={focus} width={width} height={height - 30} onDrill={drill} metric={metric} />
          : <Treemap focus={focus} width={width} height={height - 30} onDrill={drill} />}
      </div>
    </div>
  );
}

// ── sunburst ────────────────────────────────────────────────────────────────
function Sunburst({ focus, width, height, onDrill, metric }: { focus: HN; width: number; height: number; onDrill: (n: HN) => void; metric: string }) {
  const [hovered, setHovered] = useState<HN | null>(null);
  const radius = Math.max(60, Math.min(width, height) / 2 - 10);
  const laid = useMemo(() => {
    const r = focus.copy().sum((d) => (d.value != null ? Math.sqrt(d.value + 1) : 0)).sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
    partition<TreeNode>().size([2 * Math.PI, radius])(r);
    return r;
  }, [focus, radius]);
  const arc = d3arc<HN>().startAngle((d) => (d as unknown as { x0: number }).x0).endAngle((d) => (d as unknown as { x1: number }).x1)
    .innerRadius((d) => (d as unknown as { y0: number }).y0).outerRadius((d) => (d as unknown as { y1: number }).y1).padAngle(0.004).padRadius(radius / 3);
  const nodes = laid.descendants().filter((d) => d.depth > 0 && d.depth <= 3);
  const color = makeColor(laid);
  return (
    <svg width={width} height={height} className="block">
      <g transform={`translate(${width / 2},${height / 2})`}>
        {nodes.map((n) => {
          const b = n as unknown as { x0: number; x1: number; y0: number; y1: number };
          // Every wedge (sessions included) gets an inline radial label when it's
          // big enough to hold text; the rest stay reachable via the hover title
          // + the center readout (no wedge is anonymous — the map principle).
          const lbl = sunburstLabelLayout(b);
          const name = cleanHarnessPreview(n.data.name).split(/[/]/).pop() ?? "";
          const isHovered = hovered?.data.key === n.data.key;
          const baseOpacity = n.data.kind === "session" ? 0.62 : 0.9;
          return (
            <g
              key={n.data.key}
              className="cursor-pointer"
              onClick={() => onDrill(n)}
              onMouseEnter={() => setHovered(n)}
              onMouseLeave={() => setHovered((h) => (h?.data.key === n.data.key ? null : h))}
            >
              <path
                d={arc(n) ?? undefined}
                fill={color(n)}
                fillOpacity={isHovered ? 1 : baseOpacity}
                stroke={isHovered ? "#c0caf5" : "#16171f"}
                strokeWidth={isHovered ? 1.5 : 0.75}
              >
                <title>{`${cleanHarnessPreview(n.data.name)} · ${Math.round((n.value ?? 0) ** 2).toLocaleString()} ${metric}`}</title>
              </path>
              {lbl.show && name && (
                <text
                  transform={`rotate(${lbl.angleDeg}) translate(${lbl.innerR},0)${lbl.flip ? " rotate(180)" : ""}`}
                  textAnchor={lbl.flip ? "end" : "start"}
                  dominantBaseline="central"
                  fontSize={9.5} fill="#1a1b26" fontWeight={600}
                  className="pointer-events-none select-none"
                >
                  {name.slice(0, lbl.maxChars)}
                </text>
              )}
            </g>
          );
        })}
        <circle r={40} fill="#1a1b26" className="cursor-pointer" onClick={() => onDrill(focus.parent ?? focus)} />
        {(() => {
          const c = sunburstCenterText(
            hovered ? { name: cleanHarnessPreview(hovered.data.name).split(/[/]/).pop() ?? hovered.data.name, value: (hovered.value ?? 0) ** 2 } : null,
            { name: cleanHarnessPreview(focus.data.name), depth: focus.depth },
            metric,
          );
          return (
            <g className="pointer-events-none select-none">
              <text textAnchor="middle" y={-2} fontSize={11} fontWeight={600} fill="#c0caf5">{c.primary}</text>
              <text textAnchor="middle" y={13} fontSize={9} fill="#565f89">{c.secondary}</text>
            </g>
          );
        })()}
      </g>
    </svg>
  );
}

// ── treemap ───────────────────────────────────────────────────────────────
function Treemap({ focus, width, height, onDrill }: { focus: HN; width: number; height: number; onDrill: (n: HN) => void }) {
  const laid = useMemo(() => {
    const r = focus.copy().sum((d) => (d.value != null ? Math.sqrt(d.value + 1) : 0)).sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
    treemap<TreeNode>().tile(treemapSquarify).size([width, height]).paddingInner(1).paddingTop(14).round(true)(r);
    return r;
  }, [focus, width, height]);
  const cells = laid.descendants().filter((d) => d.depth > 0);
  const color = makeColor(laid);
  return (
    <svg width={width} height={height} className="block">
      {cells.map((n) => {
        const b = n as unknown as { x0: number; x1: number; y0: number; y1: number };
        const w = b.x1 - b.x0, h = b.y1 - b.y0;
        if (w < 1 || h < 1) return null;
        const leaf = n.data.kind === "session";
        return (
          <g key={n.data.key} data-tm-node={n.data.key} transform={`translate(${b.x0},${b.y0})`} className="cursor-pointer" onClick={() => onDrill(n)}>
            <rect width={w} height={h} fill={color(n)} fillOpacity={leaf ? 0.62 : 0.22} stroke="#16171f" strokeWidth={leaf ? 0.5 : 1}>
              <title>{`${cleanHarnessPreview(n.data.name)} · ${Math.round((n.value ?? 0) ** 2)}`}</title>
            </rect>
            {!leaf && w > 44 && h > 14 && (
              <text x={3} y={10} fontSize={9} fill="#c0caf5">{cleanHarnessPreview(n.data.name).split(/[/]/).pop()?.slice(0, Math.floor(w / 6))}</text>
            )}
            {leaf && w > 54 && h > 24 && (
              <text x={4} y={12} fontSize={8.5} fontWeight={500} fill="#16171f" className="pointer-events-none">
                {cleanHarnessPreview(n.data.name).split(/[/]/).pop()?.slice(0, Math.floor(w / 5.5))}
              </text>
            )}
          </g>
        );
      })}
    </svg>
  );
}
