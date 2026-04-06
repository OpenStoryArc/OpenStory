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
 */

import React, { useState, useEffect } from "react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";
import { detectLanguage } from "@/lib/detect-language";
import { stripAnsi } from "@/lib/strip-ansi";
import { extractCycles, type EvalApplyCycle, type CycleTool } from "@/lib/eval-apply";
import type { WireRecord } from "@/types/wire-record";

const DEPTH_COLORS = [
  { border: "#7aa2f7", bg: "#7aa2f718", label: "main" },  // depth 0
  { border: "#ff9e64", bg: "#ff9e6418", label: "sub" },    // depth 1
  { border: "#bb9af7", bg: "#bb9af718", label: "sub" },    // depth 2+
];

interface CycleCardProps {
  cycle: EvalApplyCycle;
  sessionId: string;
  depth?: number;
}

export function CycleCard({ cycle, sessionId, depth = 0 }: CycleCardProps) {
  const colors = DEPTH_COLORS[Math.min(depth, 2)]!;
  const [expanded, setExpanded] = useState(false);

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
            {colors.label} {sessionId.replace("agent-", "").slice(0, 8)}
          </span>
          <span className="text-[10px] text-[#565f89]">cycle {cycle.cycleNumber}</span>
          {cycle.tools.length > 0 && (
            <span className="text-[10px] text-[#565f89]">{cycle.tools.length} tools</span>
          )}
        </div>
        {cycle.isTerminal && (
          <span className="text-[9px] px-1.5 py-0.5 rounded font-bold uppercase bg-[#9ece6a18] text-[#9ece6a] border border-[#9ece6a33]">
            terminal
          </span>
        )}
      </div>

      {/* EVAL phase */}
      <div className="mx-3 mb-1 py-1.5 px-2.5 rounded-r bg-[#24283b]" style={{ borderLeft: "3px solid #9ece6a" }}>
        <span className="text-[9px] font-bold uppercase tracking-wide text-[#9ece6a]">eval</span>
        <div className="text-[11px] text-[#a9b1d6] mt-0.5">
          {cycle.evalText.slice(0, 150)}{cycle.evalText.length > 150 ? "..." : ""}
        </div>
      </div>

      {/* APPLY phase */}
      {cycle.tools.length > 0 && (
        <div className="mx-3 mb-2 py-1.5 px-2.5 rounded-r bg-[#24283b]" style={{ borderLeft: "3px solid #e0af68" }}>
          <span className="text-[9px] font-bold uppercase tracking-wide text-[#e0af68]">
            apply ({cycle.tools.length})
          </span>
          {cycle.tools.map((tool, i) => (
            <ToolRow key={i} tool={tool} sessionId={sessionId} depth={depth} />
          ))}
        </div>
      )}
    </div>
  );
}

function ToolRow({ tool, sessionId, depth }: { tool: CycleTool; sessionId: string; depth: number }) {
  const [agentExpanded, setAgentExpanded] = useState(false);
  const [agentCycles, setAgentCycles] = useState<EvalApplyCycle[] | null>(null);

  const isAgent = tool.name === "Agent";

  // Lazy fetch subagent records on expand
  useEffect(() => {
    if (!isAgent || !agentExpanded || agentCycles !== null) return;

    // The tool summary is the agent description. We need to find the agent session.
    // For now, we don't have the agent_id on the tool. We'd need it from tool_outcome.
    // TODO: pass agent_id through the cycle extraction
    // For now, show a placeholder
    setAgentCycles([]);
  }, [isAgent, agentExpanded, agentCycles]);

  return (
    <div className="mt-1">
      <div className="flex items-center gap-2 text-[11px]">
        <span className="text-[#e0af68] font-bold min-w-[40px]">{tool.name}</span>
        <span className="text-[#a9b1d6]">{tool.summary}</span>
      </div>
      {isAgent && (
        <button
          onClick={() => setAgentExpanded(!agentExpanded)}
          className="text-[10px] text-[#ff9e64] hover:text-[#c0caf5] transition-colors mt-1 ml-[48px]"
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
              sessionId={`agent-${tool.summary.slice(0, 8)}`}
              depth={depth + 1}
            />
          ))}
        </div>
      )}
      {isAgent && agentExpanded && agentCycles && agentCycles.length === 0 && (
        <div className="text-[10px] text-[#565f89] italic mt-1 ml-[48px]">
          subagent cycles not yet loaded (needs agent_id → records fetch)
        </div>
      )}
    </div>
  );
}

/** Render a list of cycles from a subagent session. */
export function CycleList({ cycles, sessionId, depth = 1 }: {
  cycles: EvalApplyCycle[];
  sessionId: string;
  depth?: number;
}) {
  return (
    <div className="space-y-1">
      {cycles.map((c) => (
        <CycleCard key={c.cycleNumber} cycle={c} sessionId={sessionId} depth={depth} />
      ))}
    </div>
  );
}
