/** Shapes tab — a live dashboard you watch shapes flow into in real time.
 *
 *  Pure sink: subscribes to the server's `shapes` broadcast (folded by
 *  `buildShapesStream$`) and renders running per-session counts + a live feed.
 *  No analysis here — the Rust Shapes actor already did it. */

import { useMemo } from "react";
import { wsMessages$ } from "@/streams/connection";
import { buildShapesStream$, EMPTY_SHAPES_STATE } from "@/streams/shapes";
import { useObservable } from "@/hooks/use-observable";
import type { ShapeCounts, ShapeRow } from "@/types/websocket";

export function ShapesView() {
  // Memoize so we don't resubscribe on every render.
  const shapes$ = useMemo(() => buildShapesStream$(wsMessages$()), []);
  const state = useObservable(shapes$, EMPTY_SHAPES_STATE);

  const sessions = Object.entries(state.counts).sort(
    (a, b) => total(b[1]) - total(a[1]),
  );

  return (
    <div className="flex-1 min-h-0 overflow-y-auto p-4" data-testid="shapes-view">
      <div className="mb-3 flex items-center gap-2">
        <h2 className="text-sm font-semibold text-[#c0caf5]">Shapes</h2>
        <span className="text-[11px] text-[#565f89]">
          live · {sessions.length} {sessions.length === 1 ? "session" : "sessions"}
        </span>
        <span className="inline-block w-1.5 h-1.5 rounded-full bg-[#9ece6a] animate-pulse" />
      </div>

      {sessions.length === 0 ? (
        <div className="text-xs text-[#565f89]">
          Waiting for shape activity… run an agent session and watch the counts tick up.
        </div>
      ) : (
        <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
          {sessions.map(([sid, counts]) => (
            <SessionCard key={sid} sessionId={sid} counts={counts} />
          ))}
        </div>
      )}

      <RecentFeed rows={state.recent} />
    </div>
  );
}

function total(c: ShapeCounts): number {
  return c.bash + c.path + c.change;
}

function topN(rec: Readonly<Record<string, number>>, n = 6): [string, number][] {
  return Object.entries(rec)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, n);
}

function SessionCard({ sessionId, counts }: { sessionId: string; counts: ShapeCounts }) {
  return (
    <div className="rounded border border-[#2f3348] bg-[#24283b] p-3">
      <div className="mb-2 flex items-center justify-between">
        <span className="font-mono text-[11px] text-[#a9b1d6]">{sessionId.slice(0, 8)}</span>
        <span className="text-[10px] text-[#565f89]">
          {counts.bash} bash · {counts.path} path · {counts.change} change
        </span>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <Tally title="Programs" items={topN(counts.programs)} accent="#7aa2f7" />
        <Tally title="Segments" items={topN(counts.top_segments)} accent="#9ece6a" />
      </div>
      <div className="mt-2 flex gap-3 text-[11px]">
        <span className="text-[#565f89]">Δ</span>
        <span className="text-[#9ece6a]">+{counts.lines_added}</span>
        <span className="text-[#f7768e]">−{counts.lines_removed}</span>
      </div>
    </div>
  );
}

function Tally({ title, items, accent }: { title: string; items: [string, number][]; accent: string }) {
  return (
    <div>
      <div className="mb-1 text-[10px] text-[#565f89]">{title}</div>
      {items.length === 0 ? (
        <div className="text-[11px] text-[#565f89]">—</div>
      ) : (
        <div className="space-y-0.5">
          {items.map(([label, count]) => (
            <div key={label} className="flex items-center gap-2 text-xs">
              <span className="min-w-0 flex-1 truncate font-mono text-[#a9b1d6]">{label}</span>
              <span className="shrink-0 text-[10px]" style={{ color: accent }}>{count}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function RecentFeed({ rows }: { rows: readonly ShapeRow[] }) {
  if (rows.length === 0) return null;
  return (
    <div className="mt-4">
      <div className="mb-1.5 text-[10px] text-[#565f89]">Recent shapes (live)</div>
      <div className="space-y-0.5">
        {rows.slice(0, 30).map((r) => (
          <div key={r.id} className="flex items-center gap-2 text-xs">
            <span className="w-[88px] shrink-0 text-[10px] text-[#565f89]">{r.shape_type}</span>
            <span className="min-w-0 flex-1 truncate font-mono text-[#a9b1d6]">{summarize(r)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function summarize(r: ShapeRow): string {
  const d = r.data as Record<string, unknown>;
  if (r.shape_type === "bash-shape") return String(d.program ?? "") + (d.subcommand ? ` ${d.subcommand}` : "");
  if (r.shape_type === "path-shape") return String(d.path ?? "");
  if (r.shape_type === "change-shape") return `${d.path ?? ""} (+${d.lines_added ?? 0}/−${d.lines_removed ?? 0})`;
  return r.shape_type;
}
