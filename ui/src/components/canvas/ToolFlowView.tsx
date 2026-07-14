/** ToolFlowView — the "grammar of agent work": a bipartite tool->tool sankey.
 *  Samples the most recent N sessions of a chosen agent, fetches their records,
 *  counts ordered tool_call->tool_call transitions, and draws from-tools (left)
 *  -> to-tools (right) as ribbons whose width = transition count. Reveals each
 *  agent's characteristic workflow (Read->Edit->Bash, retry loops, ...). */

import { useEffect, useMemo, useState } from "react";
import type { StorySession } from "@/lib/story-api";
import type { WireRecord } from "@/types/wire-record";
import type { ToolCall } from "@/types/view-record";
import { buildToolFlow, linkActive, type FlowHover } from "@/lib/tool-flow";
import { toolColor } from "@/lib/tool-colors";
import { agentColor } from "@/lib/agent-color";

interface Props {
  sessions: readonly StorySession[];
  width: number;
  height: number;
  onOpenSession: (id: string) => void;
}

const SAMPLE = 16;

export function ToolFlowView({ sessions, width, height }: Props) {
  const agents = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of sessions) counts.set(s.origin_agent || "unknown", (counts.get(s.origin_agent || "unknown") ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).map((e) => e[0]);
  }, [sessions]);
  const [agent, setAgent] = useState<string>("claude-code");
  useEffect(() => { if (agents.length && !agents.includes(agent)) setAgent(agents[0]!); }, [agents]); // eslint-disable-line react-hooks/exhaustive-deps

  const sample = useMemo(
    () => sessions.filter((s) => (s.origin_agent || "unknown") === agent)
      .slice().sort((a, b) => (b.last_event ? Date.parse(b.last_event) : 0) - (a.last_event ? Date.parse(a.last_event) : 0))
      .slice(0, SAMPLE),
    [sessions, agent],
  );

  const [sequences, setSequences] = useState<string[][]>([]);
  const [loading, setLoading] = useState(true);
  const [timedOut, setTimedOut] = useState(0);
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setTimedOut(0);

    // Each records fetch is raced against an 8s AbortController timeout so a
    // single huge session (e.g. a live one with thousands of events) can't hang
    // the whole view — the P0 "perpetual loading…" bug. allSettled means one
    // slow/failed fetch degrades gracefully instead of blocking the rest.
    const fetchRecords = async (id: string): Promise<WireRecord[] | null> => {
      const ctrl = new AbortController();
      const timer = setTimeout(() => ctrl.abort(), 8000);
      try {
        const r = await fetch(`/api/sessions/${id}/records`, { signal: ctrl.signal });
        const j = await r.json();
        return Array.isArray(j) ? (j as WireRecord[]) : [];
      } catch {
        return null; // aborted or errored
      } finally {
        clearTimeout(timer);
      }
    };

    Promise.all(sample.map((s) => fetchRecords(s.session_id))).then((all) => {
      if (cancelled) return;
      const dropped = all.filter((r) => r === null).length;
      const seqs = all
        .map((recs) => (recs ?? []).filter((r) => r.record_type === "tool_call").map((r) => (r.payload as ToolCall)?.name || "?"))
        .filter((s) => s.length > 1);
      setSequences(seqs);
      setTimedOut(dropped);
      setLoading(false);
    });
    return () => { cancelled = true; };
  }, [sample]);

  const M = { top: 28, bottom: 10, left: 8, right: 8 };
  const plotH = Math.max(height - M.top - M.bottom, 60);
  const flow = useMemo(() => buildToolFlow(sequences, { height: plotH, minCount: 2, topN: 9 }), [sequences, plotH]);

  const leftX = 150, rightX = width - 150, midX = (leftX + rightX) / 2;
  const yOff = M.top;

  const [hover, setHover] = useState<FlowHover | null>(null);
  const nodeActive = (side: "from" | "to", tool: string) =>
    !hover || flow.links.some((l) => linkActive(l, hover) && (side === "from" ? l.from : l.to) === tool);

  return (
    <div className="relative min-h-0 flex-1 bg-[#16171f]">
      <div className="flex items-center gap-2 px-3 py-1.5 text-[11px] text-[color:var(--text-muted)]">
        <span>tool grammar ·</span>
        {agents.slice(0, 6).map((a) => (
          <button key={a} onClick={() => setAgent(a)} className={`rounded px-1.5 py-0.5 text-[11px] ${agent === a ? "text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]"}`} style={agent === a ? { background: agentColor(a) } : undefined}>{a}</button>
        ))}
        <span className="ml-2">{loading ? "sampling recent sessions…" : `${sequences.length} sessions · ${flow.total} transitions${timedOut ? ` · ${timedOut} skipped (too large)` : ""}`}</span>
        {!loading && flow.total > 0 && <span className="text-[color:var(--border)]">· hover a ribbon or tool to trace its path</span>}
      </div>
      {!loading && flow.total === 0 ? (
        <div className="flex h-40 items-center justify-center text-[12px] text-[color:var(--text-muted)]">No tool transitions for {agent} (may not log tool calls).</div>
      ) : (
        <svg width={width} height={height} className="block">
          {/* ribbons */}
          {flow.links.map((l) => {
            const sy = yOff + l.sy, ty = yOff + l.ty, w = l.width;
            const d = `M${leftX},${sy - w / 2} C${midX},${sy - w / 2} ${midX},${ty - w / 2} ${rightX},${ty - w / 2}`
              + ` L${rightX},${ty + w / 2} C${midX},${ty + w / 2} ${midX},${sy + w / 2} ${leftX},${sy + w / 2} Z`;
            const active = linkActive(l, hover);
            const base = l.from === l.to ? 0.25 : 0.32;
            return <path
              key={`${l.from}-${l.to}`} d={d} fill={toolColor(l.from)}
              fillOpacity={hover ? (active ? 0.7 : 0.05) : base}
              stroke="none" className="cursor-pointer transition-opacity"
              onMouseEnter={() => setHover({ type: "link", from: l.from, to: l.to })}
              onMouseLeave={() => setHover(null)}
            >
              <title>{`${l.from} → ${l.to} · ${l.value}`}</title>
            </path>;
          })}
          {/* from nodes (left) */}
          {flow.fromNodes.map((n) => {
            const on = nodeActive("from", n.tool);
            return (
            <g key={`f-${n.tool}`} opacity={on ? 1 : 0.25} className="cursor-pointer"
               onMouseEnter={() => setHover({ type: "from", tool: n.tool })} onMouseLeave={() => setHover(null)}>
              <rect x={leftX - 8} y={yOff + n.y0} width={8} height={Math.max(n.y1 - n.y0, 1)} fill={toolColor(n.tool)} />
              <text x={leftX - 12} y={yOff + (n.y0 + n.y1) / 2 + 3} textAnchor="end" fontSize={10} fill="#c0caf5">{n.tool}</text>
            </g>
          );})}
          {/* to nodes (right) */}
          {flow.toNodes.map((n) => {
            const on = nodeActive("to", n.tool);
            return (
            <g key={`t-${n.tool}`} opacity={on ? 1 : 0.25} className="cursor-pointer"
               onMouseEnter={() => setHover({ type: "to", tool: n.tool })} onMouseLeave={() => setHover(null)}>
              <rect x={rightX} y={yOff + n.y0} width={8} height={Math.max(n.y1 - n.y0, 1)} fill={toolColor(n.tool)} />
              <text x={rightX + 12} y={yOff + (n.y0 + n.y1) / 2 + 3} textAnchor="start" fontSize={10} fill="#c0caf5">{n.tool}</text>
            </g>
          );})}
          <text x={leftX - 8} y={18} textAnchor="end" fontSize={9} fill="#565f89">from</text>
          <text x={rightX + 8} y={18} textAnchor="start" fontSize={9} fill="#565f89">to</text>
        </svg>
      )}
    </div>
  );
}
