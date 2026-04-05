/** TurnCard — one step of the coalgebra, rendered as a card.
 *
 * Ported from story_html.py / render-html.ts prototype.
 * Receives a turn.sentence PatternEvent with enriched metadata
 * containing the full StructuralTurn data.
 */

import { useState } from "react";

interface TurnCardProps {
  pattern: {
    label: string;
    metadata?: Readonly<Record<string, unknown>>;
  };
}

export function TurnCard({ pattern }: TurnCardProps) {
  const m = pattern.metadata ?? {};
  const turn = (m.turn as number) ?? 0;
  const isTerminal = (m.is_terminal as boolean) ?? true;
  const scopeDepth = (m.scope_depth as number) ?? 0;
  const envSize = (m.env_size as number) ?? 0;
  const envDelta = (m.env_delta as number) ?? 0;
  const stopReason = (m.stop_reason as string) ?? "end_turn";
  const durationMs = m.duration_ms as number | null;
  const human = m.human as { content: string; timestamp: string } | null;
  const thinking = m.thinking as { summary: string } | null;
  const eval_ = m.eval as { content: string; decision: string; stop_reason?: string } | null;
  const applies = (m.applies as Array<{
    tool_name: string;
    input_summary: string;
    output_summary: string;
    is_error: boolean;
    is_agent: boolean;
    tool_outcome?: { type: string; path?: string; command?: string; succeeded?: boolean };
  }>) ?? [];

  const depthIndent = Math.min(scopeDepth * 16, 48);

  return (
    <div
      className="mb-2 rounded-lg bg-[#1f2335] border border-[#2a2e42] overflow-hidden hover:border-[#3b4261] transition-colors"
      style={{ marginLeft: `${depthIndent}px` }}
    >
      {/* Header */}
      <div className="flex justify-between items-center px-3.5 py-2 bg-[#24283b]">
        <span className="text-[#7aa2f7] font-bold text-xs font-mono">Turn {turn}</span>
        <span className={`text-[9px] px-1.5 py-0.5 rounded font-bold uppercase tracking-wide ${
          isTerminal
            ? "bg-[#9ece6a18] text-[#9ece6a] border border-[#9ece6a33]"
            : "bg-[#e0af6818] text-[#e0af68] border border-[#e0af6833]"
        }`}>
          {isTerminal ? "terminate" : "continue"}
        </span>
      </div>

      {/* Body */}
      <div className="px-3.5 py-2.5 space-y-1.5">
        {/* Sentence one-liner */}
        <p className="text-[13px] italic text-[#c0caf5] border-b border-[#2a2e42] pb-2">
          {pattern.label}
        </p>

        {/* Domain event badges */}
        {applies.length > 0 && <DomainStrip applies={applies} />}

        {/* Human phase */}
        {human?.content && (
          <PhaseBlock label="human" color="#7dcfff">
            <ExpandableContent text={human.content} />
          </PhaseBlock>
        )}

        {/* Thinking phase */}
        {thinking?.summary && (
          <PhaseBlock label="thinking" color="#bb9af7">
            <ExpandableContent text={thinking.summary} maxHeight={40} />
          </PhaseBlock>
        )}

        {/* Eval phase */}
        {eval_ && (
          <PhaseBlock label="eval" color="#9ece6a">
            <span className={`inline-block text-[9px] px-1 py-0.5 rounded ml-1 ${
              eval_.decision === "text_only"
                ? "bg-[#9ece6a22] text-[#9ece6a]"
                : "bg-[#e0af6822] text-[#e0af68]"
            }`}>
              {eval_.decision === "text_only" ? "text" : "tool use"}
            </span>
            <ExpandableContent text={eval_.content || "(empty)"} />
          </PhaseBlock>
        )}

        {/* Apply phases */}
        {applies.slice(0, 5).map((apply, i) => (
          <PhaseBlock
            key={i}
            label={apply.is_agent ? "apply \u00b7 compound" : "apply"}
            color={apply.is_agent ? "#ff9e64" : "#e0af68"}
          >
            <span className="float-right text-[10px] text-[#565f89]">
              {apply.tool_name}
              {apply.tool_outcome && <OutcomeBadge outcome={apply.tool_outcome} />}
            </span>
            <ExpandableContent text={apply.input_summary} />
            {apply.output_summary && (
              <div className="text-[11px] text-[#565f89] mt-0.5">
                → {apply.output_summary.slice(0, 200)}
              </div>
            )}
          </PhaseBlock>
        ))}
        {applies.length > 5 && (
          <div className="text-[11px] text-[#565f89] px-2.5">
            ... and {applies.length - 5} more applies
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="flex justify-between px-3.5 py-1.5 text-[11px] text-[#565f89]">
        <span>
          env: {envSize} messages
          {envDelta > 0 && <span className="text-[#9ece6a]"> (+{envDelta})</span>}
        </span>
        <span className={isTerminal ? "text-[#9ece6a]" : "text-[#e0af68]"}>
          {stopReason} → {isTerminal ? "TERMINATE" : "CONTINUE"}
          {applies.length > 0 && ` · ${applies.length} applies`}
          {durationMs != null && ` · ${Math.round(durationMs)}ms`}
        </span>
      </div>
    </div>
  );
}

function PhaseBlock({ label, color, children }: {
  label: string;
  color: string;
  children: React.ReactNode;
}) {
  return (
    <div
      className="py-1.5 px-2.5 my-1 rounded-r bg-[#24283b]"
      style={{ borderLeft: `3px solid ${color}` }}
    >
      <span className="text-[10px] font-bold uppercase tracking-wide" style={{ color }}>
        {label}
      </span>
      <div className="mt-0.5">{children}</div>
    </div>
  );
}

function ExpandableContent({ text, maxHeight = 60 }: { text: string; maxHeight?: number }) {
  const [expanded, setExpanded] = useState(false);
  const isLong = text.length > 200;

  return (
    <div
      className="text-[12px] text-[#a9b1d6] mt-0.5 whitespace-pre-wrap break-words cursor-pointer overflow-hidden"
      style={{ maxHeight: expanded ? "none" : `${maxHeight}px` }}
      onClick={() => isLong && setExpanded(!expanded)}
    >
      {text}
    </div>
  );
}

function DomainStrip({ applies }: { applies: Array<{ tool_outcome?: { type: string; path?: string; command?: string; succeeded?: boolean } }> }) {
  const facts = applies
    .filter(a => a.tool_outcome)
    .map((a, i) => {
      const o = a.tool_outcome!;
      switch (o.type) {
        case "FileCreated":
          return <span key={i} className="inline-block px-1.5 py-0.5 rounded text-[10px] bg-[#9ece6a18] text-[#9ece6a]">+{shortPath(o.path)}</span>;
        case "FileModified":
          return <span key={i} className="inline-block px-1.5 py-0.5 rounded text-[10px] bg-[#e0af6818] text-[#e0af68]">~{shortPath(o.path)}</span>;
        case "FileRead":
          return <span key={i} className="inline-block px-1.5 py-0.5 rounded text-[10px] bg-[#7dcfff18] text-[#7dcfff]">{shortPath(o.path)}</span>;
        case "CommandExecuted":
          return <span key={i} className={`inline-block px-1.5 py-0.5 rounded text-[10px] ${o.succeeded ? "bg-[#9ece6a18] text-[#9ece6a]" : "bg-[#f7768e18] text-[#f7768e]"}`}>{(o.command ?? "").slice(0, 30)}</span>;
        case "SearchPerformed":
          return <span key={i} className="inline-block px-1.5 py-0.5 rounded text-[10px] bg-[#bb9af718] text-[#bb9af7]">search</span>;
        case "SubAgentSpawned":
          return <span key={i} className="inline-block px-1.5 py-0.5 rounded text-[10px] bg-[#ff9e6418] text-[#ff9e64]">agent</span>;
        default:
          return null;
      }
    })
    .filter(Boolean);

  if (facts.length === 0) return null;
  return <div className="flex flex-wrap gap-1 py-1">{facts}</div>;
}

function OutcomeBadge({ outcome }: { outcome: { type: string; succeeded?: boolean } }) {
  switch (outcome.type) {
    case "FileCreated":
      return <span className="ml-1 text-[10px] text-[#9ece6a]">+created</span>;
    case "FileModified":
      return <span className="ml-1 text-[10px] text-[#e0af68]">~modified</span>;
    case "CommandExecuted":
      return <span className={`ml-1 text-[10px] ${outcome.succeeded ? "text-[#9ece6a]" : "text-[#f7768e]"}`}>
        {outcome.succeeded ? "ok" : "failed"}
      </span>;
    default:
      return null;
  }
}

function shortPath(path?: string): string {
  if (!path) return "";
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}
