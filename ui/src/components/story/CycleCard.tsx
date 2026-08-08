/** CycleCard — one eval-apply cycle, the recursive unit of agent work.
 *
 * The same component renders at every depth:
 *   Depth 0: main agent cycle (blue border)
 *   Depth 1: subagent cycle (orange border)
 *   Depth 2+: nested subagent (purple border)
 *
 * Each card shows:
 *   EVAL — what the model concluded
 *   APPLY — tools dispatched (with domain facts)
 *   Agent applies expand recursively into more CycleCards
 *
 * Expand state for nested agents is Attention-driven (`forceAgentOpen` /
 * `storyAgentOpen` keys) with local useState as a human-click fallback.
 */

import { useState, useEffect } from "react";
import type { EvalApplyCycle, CycleTool } from "@/lib/eval-apply";
import { isAgentSessionOpen } from "@/lib/hash-route";

const DEPTH_COLORS = [
  { border: "#7aa2f7", bg: "#7aa2f718", label: "main" },  // depth 0
  { border: "#ff9e64", bg: "#ff9e6418", label: "sub" },    // depth 1
  { border: "#bb9af7", bg: "#bb9af718", label: "sub" },    // depth 2+
];

interface CycleCardProps {
  cycle: EvalApplyCycle;
  sessionId: string;
  depth?: number;
  /** Agent session ids forced open by Attention/hash (`&agents=`). */
  forceAgentOpen?: readonly string[];
}

export function CycleCard({
  cycle,
  sessionId,
  depth = 0,
  forceAgentOpen,
}: CycleCardProps) {
  const colors = DEPTH_COLORS[Math.min(depth, 2)]!;

  return (
    <div
      className="mb-1 rounded overflow-hidden"
      style={{ borderLeft: `3px solid ${colors.border}`, background: colors.bg }}
    >
      {/* Cycle header */}
      <div className="flex justify-between items-center px-3 py-1.5">
        <div className="flex items-center gap-2">
          <span className="text-[9px] font-mono px-1 py-0.5 rounded"
            style={{ color: colors.border, background: `${colors.border}20`, border: `1px solid ${colors.border}33` }}>
            {colors.label} {sessionId.replace("agent-", "")}
          </span>
          <span className="text-[10px] text-[color:var(--text-muted)]">cycle {cycle.cycleNumber}</span>
          {cycle.tools.length > 0 && (
            <span className="text-[10px] text-[color:var(--text-muted)]">{cycle.tools.length} tools</span>
          )}
        </div>
        {cycle.isTerminal && (
          <span className="text-[9px] px-1.5 py-0.5 rounded font-bold uppercase bg-[color:var(--green)]/9 text-[color:var(--green)] border border-[color:var(--green)]/20">
            terminal
          </span>
        )}
      </div>

      {/* EVAL phase */}
      <div className="mx-3 mb-1 py-1.5 px-2.5 rounded-r bg-[color:var(--bg-surface)]" style={{ borderLeft: "3px solid #9ece6a" }}>
        <span className="text-[9px] font-bold uppercase tracking-wide text-[color:var(--green)]">eval</span>
        <div className="text-[11px] text-[color:var(--text-bright)] mt-0.5">
          {cycle.evalText.slice(0, 150)}{cycle.evalText.length > 150 ? "..." : ""}
        </div>
      </div>

      {/* APPLY phase */}
      {cycle.tools.length > 0 && (
        <div className="mx-3 mb-2 py-1.5 px-2.5 rounded-r bg-[color:var(--bg-surface)]" style={{ borderLeft: "3px solid #e0af68" }}>
          <span className="text-[9px] font-bold uppercase tracking-wide text-[color:var(--orange)]">
            apply ({cycle.tools.length})
          </span>
          {cycle.tools.map((tool, i) => (
            <ToolRow
              key={i}
              tool={tool}
              sessionId={sessionId}
              depth={depth}
              forceAgentOpen={forceAgentOpen}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ToolRow({
  tool,
  sessionId: _sessionId,
  depth,
  forceAgentOpen,
}: {
  tool: CycleTool;
  sessionId: string;
  depth: number;
  forceAgentOpen?: readonly string[];
}) {
  // Nested Agent tools only force-open when cycle extraction carries an agent id
  // (optional extension on CycleTool). Primary Attention expand path is TurnCard
  // AgentExpand, which has tool_outcome.agent_id.
  const agentId = (tool as CycleTool & { agentId?: string }).agentId;
  const agentSessionId = agentId ? `agent-${agentId}` : null;
  const forceOpen = isAgentSessionOpen(forceAgentOpen, agentSessionId);
  const [agentExpanded, setAgentExpanded] = useState(forceOpen);
  const [agentCycles, setAgentCycles] = useState<EvalApplyCycle[] | null>(null);

  const isAgent = tool.name === "Agent";

  useEffect(() => {
    if (forceOpen) setAgentExpanded(true);
  }, [forceOpen]);

  // Lazy fetch subagent records on expand
  useEffect(() => {
    if (!isAgent || !agentExpanded || agentCycles !== null) return;
    if (!agentSessionId) {
      // Needs agent_id on the tool for a real fetch path.
      setAgentCycles([]);
      return;
    }
    fetch(`/api/sessions/${agentSessionId}/records`)
      .then((res) => res.json())
      .then(async (records) => {
        const { extractCycles } = await import("@/lib/eval-apply");
        setAgentCycles(extractCycles(records));
      })
      .catch(() => setAgentCycles([]));
  }, [isAgent, agentExpanded, agentCycles, agentSessionId]);

  return (
    <div className="mt-1">
      <div className="flex items-center gap-2 text-[11px]">
        <span className="text-[color:var(--orange)] font-bold min-w-[40px]">{tool.name}</span>
        <span className="text-[color:var(--text-bright)]">{tool.summary}</span>
      </div>
      {isAgent && (
        <button
          onClick={() => setAgentExpanded(!agentExpanded)}
          className="text-[10px] text-[color:var(--orange)] hover:text-[color:var(--text)] transition-colors mt-1 ml-[48px]"
        >
          {agentExpanded ? "▼" : "▶"} subagent: {tool.summary.slice(0, 40)}
        </button>
      )}
      {isAgent && agentExpanded && agentCycles && agentCycles.length > 0 && (
        <div className="mt-1 ml-4">
          {agentCycles.map((c) => (
            <CycleCard
              key={c.cycleNumber}
              cycle={c}
              sessionId={agentSessionId ?? `agent-${tool.summary.slice(0, 8)}`}
              depth={depth + 1}
              forceAgentOpen={forceAgentOpen}
            />
          ))}
        </div>
      )}
      {isAgent && agentExpanded && agentCycles && agentCycles.length === 0 && (
        <div className="text-[10px] text-[color:var(--text-muted)] italic mt-1 ml-[48px]">
          subagent cycles not yet loaded (needs agent_id → records fetch)
        </div>
      )}
    </div>
  );
}

/** Render a list of cycles from a subagent session. */
export function CycleList({
  cycles,
  sessionId,
  depth = 1,
  forceAgentOpen,
}: {
  cycles: EvalApplyCycle[];
  sessionId: string;
  depth?: number;
  forceAgentOpen?: readonly string[];
}) {
  return (
    <div className="space-y-1">
      {cycles.map((c) => (
        <CycleCard
          key={c.cycleNumber}
          cycle={c}
          sessionId={sessionId}
          depth={depth}
          forceAgentOpen={forceAgentOpen}
        />
      ))}
    </div>
  );
}
