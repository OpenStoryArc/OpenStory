/** TurnCard — one step of the coalgebra, rendered as a card.
 *
 * Deterministic rendering: same data → same output. Always.
 * Click-to-expand for depth: sentence diagram, collapsed applies,
 * domain event detail, eval/thinking content.
 *
 * Ported from render-html.ts prototype.
 */

import React, { useState, useMemo } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { PatternView } from "@/types/wire-record";
import { extractDomainFacts, type FactKind } from "@/lib/domain-facts";

interface TurnCardProps {
  pattern: PatternView;
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
  const verb = (m.verb as string) ?? "";
  const object = (m.object as string) ?? "";
  const adverbial = m.adverbial as string | null;
  const predicate = (m.predicate as string) ?? "answered";
  const subordinates = (m.subordinates as Array<{ role: string; verb: string; object: string; tool_calls: number }>) ?? [];
  const human = m.human as { content: string; timestamp: string } | null;
  const thinking = m.thinking as { summary: string } | null;
  const eval_ = m.eval as { content: string; decision: string; stop_reason?: string } | null;
  const applies = (m.applies as Apply[]) ?? [];

  const depthIndent = Math.min(scopeDepth * 16, 48);
  const [detailOpen, setDetailOpen] = useState(false);

  return (
    <div
      className="mb-2 rounded-lg bg-[#1f2335] border border-[#2a2e42] overflow-hidden hover:border-[#3b4261] transition-colors"
      style={{ marginLeft: `${depthIndent}px` }}
    >
      {/* Header */}
      <div className="flex justify-between items-center px-3 py-2.5 sm:px-3.5 sm:py-2 bg-[#24283b]">
        <div className="flex items-center gap-2.5 min-w-0">
          <span className="text-[10px] font-mono px-1 py-0.5 rounded bg-[#1a1b26] text-[#7aa2f7] shrink-0">
            {pattern.session_id}
          </span>
          <span className="text-[#7aa2f7] font-bold text-xs font-mono shrink-0">Turn {turn}</span>
          {pattern.events.length > 0 && (
            <span className="text-[9px] font-mono text-[#3b4261] truncate" title={pattern.events[0]}>
              {pattern.events[0]?.slice(0, 8)}
            </span>
          )}
        </div>
        <span className={`text-[9px] px-1.5 py-0.5 rounded font-bold uppercase tracking-wide ${
          isTerminal
            ? "bg-[#9ece6a18] text-[#9ece6a] border border-[#9ece6a33]"
            : "bg-[#e0af6818] text-[#e0af68] border border-[#e0af6833]"
        }`}>
          {isTerminal ? "terminate" : "continue"}
        </span>
      </div>

      {/* Always visible: diagram */}
      <div className="px-3.5 py-2.5 space-y-1">
        {/* Diagram — always shown */}
        <DiagramInline
          verb={verb}
          object={object}
          adverbial={adverbial}
          subordinates={subordinates}
          predicate={predicate}
        />

        {/* Detail toggle — everything else */}
        <button
          onClick={(e) => { e.stopPropagation(); setDetailOpen(!detailOpen); }}
          className="text-[10px] py-1 text-[#565f89] hover:text-[#7aa2f7] transition-colors"
        >
          {detailOpen ? "▼ hide detail" : "▶ detail"}
        </button>

        {detailOpen && (
          <div className="space-y-1 border-t border-[#2a2e42] pt-2 mt-1">
            {/* Sentence one-liner */}
            <p className="text-[12px] italic text-[#a9b1d6] pb-1">
              {pattern.label}
            </p>

            {/* Domain badges */}
            {applies.length > 0 && <DomainStrip applies={applies} />}

            {human?.content && (
              <PhaseBlock label="actor" color="#7dcfff">
                <ExpandableText text={human.content} />
              </PhaseBlock>
            )}

            {thinking?.summary && (
              <PhaseBlock label="thinking" color="#bb9af7">
                <ExpandableText text={thinking.summary} maxLines={2} />
              </PhaseBlock>
            )}

            {eval_ && (
              <PhaseBlock label="eval" color="#9ece6a">
                <span className={`inline-block text-[9px] px-1 py-0.5 rounded ml-1 ${
                  eval_.decision === "text_only"
                    ? "bg-[#9ece6a22] text-[#9ece6a]"
                    : "bg-[#e0af6822] text-[#e0af68]"
                }`}>
                  {eval_.decision === "text_only" ? "text" : "tool use"}
                </span>
                <ExpandableText text={eval_.content || "(empty)"} />
              </PhaseBlock>
            )}

            <ApplyList applies={applies} />
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

// ─────────────────────────────────────────────
// Sentence diagram (click to expand)
// ─────────────────────────────────────────────

function DiagramInline({ verb, object, adverbial, subordinates, predicate }: {
  verb: string; object: string; adverbial: string | null;
  subordinates: Array<{ role: string; verb: string; object: string; tool_calls: number }>;
  predicate: string;
}) {
  return (
    <div className="px-1 py-1 bg-[#1a1b26] rounded text-[11px] font-mono">
      <div>
        <span className="text-[#7aa2f7] font-bold">Claude</span>
        <span className="text-[#3b4261]"> ──── </span>
        <span className="text-[#9ece6a] font-bold">{verb}</span>
        <span className="text-[#3b4261]"> ──── </span>
        <span className="text-[#c0caf5]">{object}</span>
      </div>
      {subordinates.map((sub, i) => (
        <div key={i} className="pl-5 my-0.5">
          <span className="text-[#3b4261]">├──</span>{" "}
          <span style={{ color: ROLE_COLORS[sub.role] ?? "#565f89" }}>{sub.verb}</span>{" "}
          <span className="text-[#c0caf5]">{sub.object}</span>{" "}
          <span className="text-[#565f89]">({sub.tool_calls})</span>
        </div>
      ))}
      {adverbial && (
        <div className="pl-5 my-0.5">
          <span className="text-[#3b4261]">└──</span>{" "}
          <span className="text-[#f7768e]">because</span>{" "}
          <span className="text-[#c0caf5]">{adverbial}</span>
        </div>
      )}
      <div className="pl-5 mt-1 text-[#9ece6a]">→ {predicate}</div>
    </div>
  );
}

const ROLE_COLORS: Record<string, string> = {
  Preparatory: "#7dcfff",
  Creative: "#9ece6a",
  Verificatory: "#e0af68",
  Delegatory: "#bb9af7",
  Interactive: "#565f89",
};


// ─────────────────────────────────────────────
// Apply list — show first 2, collapse rest
// ─────────────────────────────────────────────

type Apply = {
  tool_name: string;
  input_summary: string;
  output_summary: string;
  is_error: boolean;
  is_agent: boolean;
  tool_outcome?: { type: string; path?: string; command?: string; succeeded?: boolean };
};

function ApplyList({ applies }: { applies: Apply[] }) {
  const [expanded, setExpanded] = useState(false);

  if (applies.length === 0) return null;

  const visible = applies.length <= 3 || expanded ? applies : applies.slice(0, 2);
  const hidden = applies.length > 3 && !expanded ? applies.slice(2) : [];

  // Group hidden by tool name
  const grouped: Record<string, number> = {};
  for (const a of hidden) {
    grouped[a.tool_name] = (grouped[a.tool_name] ?? 0) + 1;
  }
  const groupSummary = Object.entries(grouped)
    .map(([name, count]) => `${name} ×${count}`)
    .join(", ");

  return (
    <>
      {visible.map((apply, i) => (
        <ApplyBlock key={i} apply={apply} />
      ))}
      {hidden.length > 0 && (
        <button
          onClick={(e) => { e.stopPropagation(); setExpanded(true); }}
          className="w-full text-left py-1.5 px-2.5 my-1 rounded-r bg-[#24283b] border-l-[3px] border-[#e0af68] text-[11px] text-[#a9b1d6] hover:bg-[#2a3050] transition-colors"
        >
          ▶ ... and {hidden.length} more: <span className="text-[#e0af68]">{groupSummary}</span>
        </button>
      )}
    </>
  );
}

function ApplyBlock({ apply }: { apply: Apply }) {
  const [showOutput, setShowOutput] = useState(false);
  const cls = apply.is_agent ? "border-[#ff9e64]" : apply.is_error ? "border-[#f7768e]" : "border-[#e0af68]";
  const labelColor = apply.is_agent ? "text-[#ff9e64]" : apply.is_error ? "text-[#f7768e]" : "text-[#e0af68]";
  const label = apply.is_agent ? "apply · compound" : "apply";

  return (
    <div className={`py-1.5 px-2.5 my-1 rounded-r bg-[#24283b] border-l-[3px] ${cls}`}>
      <div className="flex justify-between items-start">
        <span className={`text-[10px] font-bold uppercase tracking-wide ${labelColor}`}>{label}</span>
        <span className="text-[10px] text-[#565f89]">
          {apply.tool_name}
          {apply.tool_outcome && <OutcomeBadge outcome={apply.tool_outcome} />}
        </span>
      </div>
      <div className="text-[12px] text-[#a9b1d6] mt-0.5 whitespace-pre-wrap break-words">
        {apply.input_summary || "(no input)"}
      </div>
      {apply.output_summary && (
        <button
          onClick={(e) => { e.stopPropagation(); setShowOutput(!showOutput); }}
          className="text-[10px] py-0.5 text-[#565f89] hover:text-[#7aa2f7] transition-colors mt-0.5"
        >
          {showOutput ? "▼ hide output" : "▶ show output"}
        </button>
      )}
      {showOutput && apply.output_summary && (
        <div className="text-[11px] text-[#565f89] mt-1 whitespace-pre-wrap break-words max-h-60 overflow-y-auto border-t border-[#2a2e42] pt-1">
          <Markdown remarkPlugins={[remarkGfm]}>{apply.output_summary}</Markdown>
        </div>
      )}
      {apply.is_agent && (
        <div className="text-[10px] text-[#565f89] italic mt-0.5">
          nested eval-apply loop with fresh scope
        </div>
      )}
    </div>
  );
}

// ─────────────────────────────────────────────
// Domain event strip — aggregated
// ─────────────────────────────────────────────

const FACT_STYLES: Record<FactKind, { icon: string; color: string; bg: string }> = {
  created:      { icon: "+", color: "#9ece6a", bg: "#9ece6a18" },
  modified:     { icon: "~", color: "#e0af68", bg: "#e0af6818" },
  read:         { icon: "⊳", color: "#7dcfff", bg: "#7dcfff18" },
  command_ok:   { icon: "$", color: "#9ece6a", bg: "#9ece6a18" },
  command_fail: { icon: "✗", color: "#f7768e", bg: "#f7768e18" },
  search:       { icon: "⌕", color: "#bb9af7", bg: "#bb9af718" },
  agent:        { icon: "⊕", color: "#ff9e64", bg: "#ff9e6418" },
  error:        { icon: "✗", color: "#f7768e", bg: "#f7768e18" },
};

function DomainStrip({ applies }: { applies: Apply[] }) {
  const facts = useMemo(() => extractDomainFacts(applies as any), [applies]);
  const [expanded, setExpanded] = useState(false);

  if (facts.length === 0) return null;

  const visible = expanded ? facts : facts.slice(0, 6);
  const hidden = expanded ? 0 : facts.length - 6;

  return (
    <div className="py-1">
      <div className="flex flex-wrap gap-1">
        {visible.map((fact, i) => {
          const style = FACT_STYLES[fact.kind];
          return (
            <span
              key={`${fact.kind}-${i}`}
              className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded text-[10px]"
              style={{ backgroundColor: style.bg, color: style.color }}
              title={fact.detail}
            >
              <span>{style.icon}</span>
              {fact.label}
            </span>
          );
        })}
        {hidden > 0 && (
          <button
            onClick={(e) => { e.stopPropagation(); setExpanded(true); }}
            className="text-[10px] text-[#565f89] hover:text-[#7aa2f7] px-1"
          >
            +{hidden} more
          </button>
        )}
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────
// Shared components
// ─────────────────────────────────────────────

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

function ExpandableText({ text, maxLines = 3 }: { text: string; maxLines?: number }) {
  const [expanded, setExpanded] = useState(false);
  const isLong = text.length > 150;
  const lineHeight = 18;
  const maxHeight = maxLines * lineHeight;

  return (
    <div className="mt-0.5">
      <div
        className="text-[12px] text-[#a9b1d6] break-words overflow-hidden prose prose-invert prose-sm max-w-none
          [&_code]:bg-[#1a1b26] [&_code]:px-1 [&_code]:rounded [&_code]:text-[11px]
          [&_pre]:bg-[#1a1b26] [&_pre]:p-2 [&_pre]:rounded [&_pre]:text-[11px] [&_pre]:overflow-x-auto
          [&_a]:text-[#7aa2f7] [&_p]:my-1 [&_ul]:my-1 [&_li]:my-0"
        style={{ maxHeight: expanded || !isLong ? "none" : `${maxHeight}px` }}
      >
        <Markdown remarkPlugins={[remarkGfm]}>{text}</Markdown>
      </div>
      {isLong && (
        <button
          onClick={(e) => { e.stopPropagation(); setExpanded(!expanded); }}
          className="text-[10px] text-[#565f89] hover:text-[#7aa2f7] mt-0.5 transition-colors"
        >
          {expanded ? "▲ collapse" : "▼ expand"}
        </button>
      )}
    </div>
  );
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
    case "FileReadFailed":
    case "FileWriteFailed":
      return <span className="ml-1 text-[10px] text-[#f7768e]">failed</span>;
    default:
      return null;
  }
}
