/** LabView (#/lab) — the falsifiable viz design-space explorer. Renders the
 *  mined candidate visualizations (docs/research/viz-candidates.json via
 *  /api/viz-candidates) as score-sorted cards, each showing its hypothesis, its
 *  falsifier, and the witness that could kill it. Phase 0: the catalog. Phase 1
 *  adds a live "run witness" per card; Phase 2 renders the built shapes here. */

import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { HashRoute } from "@/lib/hash-route";
import { fetchVizCandidates, sortByScore, type VizCandidate } from "@/lib/viz-candidates";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { runWitness, type WitnessResult } from "@/lib/witnesses";
import { controlActions$ } from "@/streams/control";
import { postInteraction } from "@/lib/interaction";
import type { StorySession } from "@/lib/story-api";
import { ToolAdjacencyHeatmap } from "@/components/canvas/ToolAdjacencyHeatmap";
import { AgentProjectMatrix } from "@/components/canvas/AgentProjectMatrix";
import { DurationBeeswarm } from "@/components/canvas/DurationBeeswarm";
import { EventsStreamgraph } from "./EventsStreamgraph";
import { ParallelCoords } from "./ParallelCoords";
import { cn } from "@/lib/cn";

/** A verdict for a card: not-run, no-runner (needs records), or a result. */
type Verdict = { ran: true; result: WitnessResult | null };

/** Built shapes, keyed by candidate id. A candidate is "built" iff it has an
 *  entry here — the registry is the source of truth for what's implemented. */
const BUILT_VIZ: Record<string, (sessions: readonly StorySession[]) => ReactNode> = {
  "tool-adjacency-heatmap": (sessions) => <ToolAdjacencyHeatmap sessions={sessions} />,
  "agent-project-matrix": (sessions) => <AgentProjectMatrix sessions={sessions} />,
  "duration-beeswarm": (sessions) => <DurationBeeswarm sessions={sessions} />,
  "events-streamgraph": (sessions) => <EventsStreamgraph sessions={sessions} />,
  "session-parallel-coords": (sessions) => <ParallelCoords sessions={sessions} />,
};

const STATUS_STYLE: Record<string, string> = {
  built: "bg-[#9ece6a]/20 text-[#9ece6a] border-[#9ece6a]/40",
  witnessed: "bg-[#7aa2f7]/20 text-[#7aa2f7] border-[#7aa2f7]/40",
  refuted: "bg-[#f7768e]/20 text-[#f7768e] border-[#f7768e]/40",
  idea: "bg-[#565f89]/20 text-[#565f89] border-[#565f89]/40",
};

function scoreColor(s: number): string {
  if (s >= 8) return "#9ece6a";
  if (s >= 6) return "#e0af68";
  return "#565f89";
}

export function LabView(_props: { onNavigate: (route: HashRoute) => void }) {
  const [candidates, setCandidates] = useState<VizCandidate[]>([]);
  const [loading, setLoading] = useState(true);
  const [verdicts, setVerdicts] = useState<Record<string, Verdict>>({});
  const [openId, setOpenId] = useState<string | null>(null);
  const { sessions } = useSessionsList();
  useEffect(() => { fetchVizCandidates().then((c) => { setCandidates(c); setLoading(false); }); }, []);
  const sorted = useMemo(() => sortByScore(candidates), [candidates]);

  // Open a built shape (records a typed interaction so an agent can see it).
  const open = (id: string) => { setOpenId(id); postInteraction({ kind: "select", view: "lab", session_id: id }); };
  // Agent-in-UI: `lab.open`=<candidate id> opens a built shape remotely.
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type === "toggle" && a.target === "lab.open" && BUILT_VIZ[a.value]) setOpenId(a.value);
    });
    return () => sub.unsubscribe();
  }, []);

  // The lab method, live: run a candidate's witness against the real sessions.
  const run = (id: string) => setVerdicts((v) => ({ ...v, [id]: { ran: true, result: runWitness(id, sessions) } }));
  const runAll = () => {
    const next: Record<string, Verdict> = {};
    for (const c of candidates) next[c.id] = { ran: true, result: runWitness(c.id, sessions) };
    setVerdicts(next);
  };
  /** effective status: a fired witness overrides the catalog status. */
  const statusOf = (c: VizCandidate): string => {
    if (BUILT_VIZ[c.id]) return "built"; // registry is the source of truth
    const v = verdicts[c.id];
    if (!v) return c.status ?? "idea";
    if (v.result === null) return c.status ?? "idea"; // no runner → unchanged
    return v.result.grounded ? "witnessed" : "refuted";
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-[#16171f] text-[#c0caf5]" data-testid="lab-view">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 border-b border-[#2f3348] bg-[#1a1b26] px-4 py-2.5">
        <span className="text-[15px] font-semibold">🧪 Lab</span>
        <span className="text-[11px] text-[#565f89]">
          the viz design-space — {candidates.length} candidate shapes, each a falsifiable claim (hypothesis · falsifier · witness). Score = novelty×insight ÷ cost.
        </span>
        <button
          onClick={runAll}
          disabled={!sessions.length}
          className="ml-auto rounded border border-[#e0af68]/50 bg-[#e0af68]/10 px-2 py-1 text-[11px] text-[#e0af68] hover:bg-[#e0af68]/20 disabled:opacity-40"
          title="Run every witness against the live session data"
        >
          ⚗ run all witnesses ({sessions.length})
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {loading ? (
          <div className="text-[12px] text-[#565f89]">Loading the catalog…</div>
        ) : sorted.length === 0 ? (
          <div className="text-[12px] text-[#565f89]">No candidates found (is /api/viz-candidates serving docs/research/viz-candidates.json?).</div>
        ) : (
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
            {sorted.map((c) => (
              <div key={c.id} data-candidate={c.id} className="flex flex-col rounded-lg border border-[#2f3348] bg-[#1a1b26] p-3">
                <div className="mb-1 flex items-start gap-2">
                  <span className="shrink-0 rounded px-1.5 py-0.5 text-[13px] font-bold tabular-nums" style={{ color: scoreColor(c.score), background: `${scoreColor(c.score)}18` }}>
                    {c.score.toFixed(1)}
                  </span>
                  <span className="min-w-0 flex-1 text-[13px] font-semibold leading-tight text-[#c0caf5]">{c.name}</span>
                  <span className={cn("shrink-0 rounded border px-1.5 py-0.5 text-[9px] uppercase", STATUS_STYLE[statusOf(c)] ?? STATUS_STYLE.idea)}>
                    {statusOf(c)}
                  </span>
                </div>
                <div className="mb-1.5 text-[10px] text-[#565f89]">{c.d3_shape} · {c.data_shape}</div>
                <div className="mb-2 text-[11px] leading-snug text-[#a9b1d6]">{c.what_it_shows}</div>
                <div className="mt-auto flex flex-col gap-1 border-t border-[#2f3348]/60 pt-2 text-[10px] leading-snug">
                  <div><span className="text-[#7aa2f7]">hypothesis</span> <span className="text-[#a9b1d6]">{c.hypothesis}</span></div>
                  <div><span className="text-[#f7768e]">falsifier</span> <span className="text-[#565f89]">{c.falsifier}</span></div>
                  <div><span className="text-[#e0af68]">witness</span> <span className="text-[#565f89]">{c.witness}</span></div>
                </div>
                <div className="mt-2 flex items-center gap-2">
                  <button
                    data-run-witness={c.id}
                    onClick={() => run(c.id)}
                    disabled={!sessions.length}
                    className="rounded border border-[#3b4261] px-2 py-0.5 text-[10px] text-[#a9b1d6] hover:border-[#e0af68] hover:text-[#e0af68] disabled:opacity-40"
                  >
                    ⚗ run witness
                  </button>
                  {BUILT_VIZ[c.id] && (
                    <button data-open-viz={c.id} onClick={() => open(c.id)}
                      className="rounded border border-[#9ece6a]/50 bg-[#9ece6a]/10 px-2 py-0.5 text-[10px] text-[#9ece6a] hover:bg-[#9ece6a]/20">
                      open ▸
                    </button>
                  )}
                  {verdicts[c.id] && (() => {
                    const r = verdicts[c.id]!.result;
                    if (r === null) return <span className="text-[10px] text-[#565f89]">needs records — no runner yet</span>;
                    return (
                      <span className={cn("text-[10px] font-medium", r.grounded ? "text-[#9ece6a]" : "text-[#f7768e]")} data-verdict={c.id}>
                        {r.grounded ? "✓ grounded" : "✗ refuted"} · {r.detail}
                      </span>
                    );
                  })()}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* built-shape viewer */}
      {openId && BUILT_VIZ[openId] && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6" onClick={() => setOpenId(null)} data-testid="lab-viewer">
          <div className="max-h-[88vh] max-w-[92vw] overflow-auto rounded-xl border border-[#2f3348] bg-[#1a1b26] shadow-2xl" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center gap-2 border-b border-[#2f3348] px-4 py-2">
              <span className="text-[13px] font-semibold text-[#9ece6a]">🧪 {sorted.find((c) => c.id === openId)?.name ?? openId}</span>
              <button onClick={() => setOpenId(null)} className="ml-auto rounded px-2 text-[#565f89] hover:text-[#c0caf5]" aria-label="Close">×</button>
            </div>
            {BUILT_VIZ[openId]!(sessions)}
          </div>
        </div>
      )}
    </div>
  );
}
