/** HeatmapView — a GitHub-style contribution heatmap of the session fleet.
 *  Increment 1: the 2D grid (the default), a facet filter bar, and time-range
 *  zoom (12/26/52 weeks). Filtering narrows the set; zoom reframes the window —
 *  both keep a lot of data legible. 3D stacks + hover/click land in later
 *  increments. Pure model in lib/heatmap.ts. */

import { useMemo, useState, useEffect, lazy, Suspense } from "react";
import type { HashRoute } from "@/lib/hash-route";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { controlActions$ } from "@/streams/control";
import { isSubagentSession } from "@/lib/subagents";
import { buildHeatmap, heatLevel } from "@/lib/heatmap";
import { computeFacets, applyFilters, type OverviewFilters, type Facets } from "@/lib/sessions-overview";
import { cn } from "@/lib/cn";

// three.js only loads when you toggle into 3D — keeps the 2D default lightweight.
const Heatmap3D = lazy(() => import("./Heatmap3D"));

const CELL = 13, GAP = 3, LEFT = 30, TOP = 18;
const STEP = CELL + GAP;
// GitHub-dark contribution scale (level 0 = empty).
const SCALE = ["#1b1f2a", "#0e4429", "#1a7f37", "#2ea043", "#39d353"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const RANGES: { label: string; weeks: number }[] = [
  { label: "12w", weeks: 12 },
  { label: "26w", weeks: 26 },
  { label: "52w", weeks: 52 },
];
const FILTER_DIMS: { key: keyof OverviewFilters; label: string; facet: keyof Facets }[] = [
  { key: "project", label: "Project", facet: "projects" },
  { key: "agent", label: "Agent", facet: "agents" },
  { key: "user", label: "User", facet: "users" },
  { key: "status", label: "Status", facet: "statuses" },
];

export function HeatmapView({ onNavigate, onOpenSession }: {
  onNavigate: (route: HashRoute) => void;
  /** 3D box click → open that session (canvas side panel when hosted there). */
  onOpenSession?: (id: string) => void;
}) {
  const { sessions, loading } = useSessionsList();
  const universe = useMemo(() => sessions.filter((s) => !isSubagentSession(s.session_id)), [sessions]);
  const nowMs = useMemo(() => Date.now(), []);
  const [weeks, setWeeks] = useState(26);
  const [filters, setFilters] = useState<OverviewFilters>({});
  const [is3D, setIs3D] = useState(true); // 3D is the default — the strongest read of the fleet

  // Agent-in-UI: apply `heatmap.*` toggle intents (component-local). Sink only.
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type !== "toggle") return;
      if (a.target === "heatmap.dim") {
        if (a.value === "3d") setIs3D(true);
        else if (a.value === "2d") setIs3D(false);
      } else if (a.target === "heatmap.weeks") {
        const w = Number(a.value);
        if (RANGES.some((r) => r.weeks === w)) setWeeks(w);
      }
    });
    return () => sub.unsubscribe();
  }, []);

  const facets = useMemo(() => computeFacets(universe), [universe]);
  const filtered = useMemo(() => applyFilters(universe, filters), [universe, filters]);
  const grid = useMemo(() => buildHeatmap(filtered, { nowMs, weeks }), [filtered, nowMs, weeks]);

  const activeDays = grid.cells.filter((c) => c.count > 0).length;
  const hasFilters = Object.values(filters).some(Boolean);
  const toggle = (dim: keyof OverviewFilters, value: string) =>
    setFilters((f) => ({ ...f, [dim]: f[dim] === value ? undefined : value }));

  const svgW = LEFT + weeks * STEP;
  const svgH = TOP + 7 * STEP;

  // month labels: mark the first week whose Sunday falls in a new month
  const monthTicks: { x: number; label: string }[] = [];
  let lastMonth = -1;
  for (let w = 0; w < weeks; w++) {
    const sun = grid.cells[w * 7];
    if (!sun) continue;
    const m = Number(sun.date.slice(5, 7)) - 1;
    if (m !== lastMonth) { monthTicks.push({ x: LEFT + w * STEP, label: MONTHS[m]! }); lastMonth = m; }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-[#16171f] text-[color:var(--text)]" data-testid="heatmap-view">
      {/* header */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-[color:var(--bg-hover)] bg-[color:var(--bg)] px-4 py-2.5">
        <div className="flex items-baseline gap-2">
          <span className="text-[15px] font-semibold">Contributions</span>
          <span className="text-[11px] text-[color:var(--text-muted)]">{grid.totalSessions.toLocaleString()} sessions · {activeDays} active days · last {weeks} weeks</span>
        </div>
        <div className="ml-auto flex items-center gap-2">
          <div className="flex items-center gap-1 rounded border border-[color:var(--border)] p-0.5">
            {RANGES.map((r) => (
              <button key={r.weeks} onClick={() => setWeeks(r.weeks)}
                className={cn("rounded px-2 py-0.5 text-[11px] transition-colors", weeks === r.weeks ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]")}>{r.label}</button>
            ))}
          </div>
          <div className="flex items-center gap-0.5 rounded border border-[color:var(--border)] p-0.5">
            {([["2D", false], ["3D", true]] as const).map(([lbl, on]) => (
              <button key={lbl} onClick={() => setIs3D(on)}
                className={cn("rounded px-2 py-0.5 text-[11px] transition-colors", is3D === on ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]")}>{lbl}</button>
            ))}
          </div>
        </div>
      </div>

      {/* filter bar */}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 border-b border-[color:var(--bg-hover)]/60 bg-[color:var(--bg)] px-4 py-2">
        {FILTER_DIMS.map((dim) => {
          const values = facets[dim.facet].slice(0, 6);
          if (!values.length) return null;
          return (
            <div key={dim.key} className="flex items-center gap-1">
              <span className="text-[9px] uppercase tracking-wide text-[color:var(--text-muted)]">{dim.label}</span>
              {values.map((v) => (
                <button key={v.key} onClick={() => toggle(dim.key, v.key)}
                  className={cn("rounded px-1.5 py-0.5 text-[11px] transition-colors", filters[dim.key] === v.key ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:bg-[color:var(--bg-hover)] hover:text-[color:var(--text)]")}>
                  {v.key}<span className="ml-1 opacity-60">{v.count}</span>
                </button>
              ))}
            </div>
          );
        })}
        {hasFilters && (
          <button onClick={() => setFilters({})} className="rounded border border-[color:var(--border)] px-2 py-0.5 text-[11px] text-[color:var(--text-muted)] hover:text-[color:var(--text)]">clear</button>
        )}
      </div>

      {/* 3D stacks */}
      {is3D && (
        <div className="relative min-h-0 flex-1">
          <Suspense fallback={<div className="flex h-full items-center justify-center text-[12px] text-[color:var(--text-muted)]">Loading 3D…</div>}>
            <Heatmap3D
              grid={grid}
              onOpenSession={(id) => (onOpenSession ? onOpenSession(id) : onNavigate({ view: "explore", sessionId: id }))}
              onDayFilter={(date) => onNavigate({ view: "explore", explore: { filters: { day: date } } })}
            />
          </Suspense>
          <div className="pointer-events-none absolute bottom-3 left-4 text-[10px] text-[color:var(--text-muted)]">
            each box = a session · warm (largest) → cool (smallest) · drag to orbit · hover a box to see it, click to open
          </div>
        </div>
      )}

      {/* 2D grid */}
      {!is3D && (
      <div className="min-h-0 flex-1 overflow-auto p-4">
        {loading ? (
          <div className="text-[12px] text-[color:var(--text-muted)]">Loading…</div>
        ) : (
          <svg width={svgW} height={svgH} className="block">
            {monthTicks.map((t, i) => (
              <text key={i} x={t.x} y={11} fontSize={9} fill="#565f89">{t.label}</text>
            ))}
            {["Mon", "Wed", "Fri"].map((d, i) => (
              <text key={d} x={0} y={TOP + (i * 2 + 1) * STEP + CELL - 3} fontSize={9} fill="#565f89">{d}</text>
            ))}
            {grid.cells.map((c) => {
              if (!c.present) return null;
              const lvl = heatLevel(c.count, grid.maxCount);
              return (
                <rect
                  key={c.date} data-heat-cell={c.date}
                  x={LEFT + c.week * STEP} y={TOP + c.day * STEP} width={CELL} height={CELL} rx={2.5}
                  fill={SCALE[lvl]}
                  className={c.count ? "cursor-pointer" : ""}
                  onClick={c.count ? () => onNavigate({ view: "explore", explore: { filters: { ...filters, day: c.date } } }) : undefined}
                >
                  <title>{c.count ? `${c.date} · ${c.count} session${c.count === 1 ? "" : "s"} · ${c.events.toLocaleString()} ev` : `${c.date} · no sessions`}</title>
                </rect>
              );
            })}
          </svg>
        )}

        {/* legend */}
        <div className="mt-3 flex items-center gap-1.5 text-[10px] text-[color:var(--text-muted)]">
          <span>Less</span>
          {SCALE.map((c, i) => <span key={i} className="inline-block h-[11px] w-[11px] rounded-[2px]" style={{ background: c }} />)}
          <span>More</span>
          <span className="ml-3">Click a day → filter Explore to it.</span>
        </div>
      </div>
      )}
    </div>
  );
}
