/** A single conversation turn: prompt → thinking → tool calls → response. */

import { useState } from "react";
import type { ConversationTurn } from "@/lib/conversation";
import type { ToolRoundtripEntry, ToolCall } from "@/types/view-record";
import { compactTime } from "@/lib/time";
import { toolColor } from "@/lib/tool-colors";
import { toolInputSummary } from "@/types/view-record";

interface TurnCardProps {
  turn: ConversationTurn;
  index: number;
}

export function TurnCard({ turn, index }: TurnCardProps) {
  const [showThinking, setShowThinking] = useState(false);
  const [expandedTools, setExpandedTools] = useState<Set<number>>(new Set());

  const toggleTool = (i: number) => {
    setExpandedTools((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };

  return (
    <div className="border border-[#2f3348] rounded-xl overflow-hidden" data-testid="turn-card">
      {/* Prompt */}
      {turn.prompt && (
        <div className="px-4 py-3 bg-[#24283b] border-b border-[#2f3348]">
          <div className="flex items-center gap-2 mb-1">
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-[#7aa2f718] text-[#7aa2f7] font-medium">
              Turn {index + 1}
            </span>
            {turn.promptTimestamp && (
              <span className="text-[10px] text-[#565f89] font-mono">
                {compactTime(turn.promptTimestamp)}
              </span>
            )}
          </div>
          <p className="text-sm text-[#c0caf5] whitespace-pre-wrap">{turn.prompt}</p>
        </div>
      )}

      {/* Thinking (collapsed by default) */}
      {turn.thinking && (
        <div className="border-b border-[#2f3348]">
          <button
            onClick={() => setShowThinking((v) => !v)}
            className="w-full px-4 py-1.5 text-left text-[11px] text-[#9ece6a] hover:bg-[#1a1b26] transition-colors flex items-center gap-1"
          >
            <span className="text-[10px]">{showThinking ? "▾" : "▸"}</span>
            <span className="italic">Thinking...</span>
          </button>
          {showThinking && (
            <div className="px-4 py-2 text-xs text-[#9ece6a] italic opacity-70 whitespace-pre-wrap">
              {turn.thinking}
            </div>
          )}
        </div>
      )}

      {/* Tool calls */}
      {turn.toolCalls.length > 0 && (
        <div className="border-b border-[#2f3348]">
          {turn.toolCalls.map((tc, i) => (
            <ToolCallRow
              key={i}
              entry={tc}
              expanded={expandedTools.has(i)}
              onToggle={() => toggleTool(i)}
            />
          ))}
        </div>
      )}

      {/* Response */}
      {turn.response && (
        <div className="px-4 py-3">
          {turn.responseTimestamp && (
            <span className="text-[10px] text-[#565f89] font-mono mb-1 block">
              {compactTime(turn.responseTimestamp)}
            </span>
          )}
          <p className="text-sm text-[#a9b1d6] whitespace-pre-wrap">
            {turn.response.length > 500 ? turn.response.slice(0, 500) + "…" : turn.response}
          </p>
        </div>
      )}
    </div>
  );
}

function ToolCallRow({ entry, expanded, onToggle }: {
  entry: ToolRoundtripEntry;
  expanded: boolean;
  onToggle: () => void;
}) {
  const call = entry.call;
  const result = entry.result;
  const tc = call.payload as ToolCall;
  const name = tc.name;
  const color = toolColor(name);
  const summary = toolInputSummary(tc.typed_input);
  const resultOutput = result?.payload ? (result.payload as { output?: string }).output : null;
  const isError = result?.payload ? (result.payload as { is_error?: boolean }).is_error : false;

  return (
    <div className="border-b border-[#2f334830] last:border-b-0">
      <button
        onClick={onToggle}
        className="w-full px-4 py-1.5 text-left text-xs hover:bg-[#1a1b26] transition-colors flex items-center gap-2"
      >
        <span className="text-[10px]">{expanded ? "▾" : "▸"}</span>
        <span
          className="w-2 h-2 rounded-full shrink-0"
          style={{ backgroundColor: color }}
        />
        <span className="font-medium" style={{ color }}>{name}</span>
        {summary && <span className="text-[#565f89] truncate">{summary}</span>}
        {isError && <span className="text-[#f7768e] text-[10px] ml-auto">error</span>}
      </button>
      {expanded && resultOutput && (
        <div className="px-4 py-2 mx-4 mb-2 rounded bg-[#1a1b26]">
          <pre className="text-[11px] text-[#a9b1d6] whitespace-pre-wrap break-words max-h-[200px] overflow-y-auto">
            {resultOutput.length > 1000 ? resultOutput.slice(0, 1000) + "\n..." : resultOutput}
          </pre>
        </div>
      )}
    </div>
  );
}
