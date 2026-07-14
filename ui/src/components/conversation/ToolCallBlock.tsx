import { memo, useState } from "react";
import type { ToolCall, ToolResult } from "@/types/view-record";
import { toolInputSummary } from "@/types/view-record";
import { truncate } from "@/lib/event-transforms";
import { stripAnsi } from "@/lib/strip-ansi";

interface ToolCallBlockProps {
  call: ToolCall;
  result?: ToolResult;
}

export const ToolCallBlock = memo(function ToolCallBlock({
  call,
  result,
}: ToolCallBlockProps) {
  const [expanded, setExpanded] = useState(false);

  const summary = truncate(toolInputSummary(call.typed_input), 60);

  return (
    <div className="mx-4 my-1 rounded border border-[color:var(--bg-hover)] bg-[color:var(--bg-surface)] text-xs">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-2 px-3 py-2 hover:bg-[color:var(--bg-hover)] transition-colors"
      >
        <span className="text-[color:var(--cyan)] font-medium">{call.name}</span>
        <span className="text-[color:var(--text-muted)] flex-1 text-left truncate">
          {summary}
        </span>
        <span className="text-[color:var(--text-muted)]">{expanded ? "\u25B2" : "\u25BC"}</span>
      </button>
      {expanded && (
        <div className="border-t border-[color:var(--bg-hover)]">
          <div className="px-3 py-2">
            <div className="text-[color:var(--text-muted)] mb-1">Input:</div>
            <pre className="bg-[color:var(--bg)] rounded p-2 overflow-x-auto whitespace-pre-wrap break-words text-[color:var(--text)]">
              {JSON.stringify(call.raw_input, null, 2)}
            </pre>
          </div>
          {result && (
            <div className="px-3 py-2 border-t border-[color:var(--bg-hover)]">
              <div className="text-[color:var(--text-muted)] mb-1">
                Result{result.is_error ? " (error)" : ""}:
              </div>
              <pre className="bg-[color:var(--bg)] rounded p-2 overflow-x-auto whitespace-pre-wrap break-words text-[color:var(--text)] max-h-60 overflow-y-auto">
                {result.output ? stripAnsi(result.output) : "(no output)"}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
});
