/** TurnTraceView — an observability-style waterfall of a session's tool calls.
 *
 *  Each tool_call→tool_result pair is a span; its bar's length is the wall-clock
 *  it took. This is the view that answers "where did the time go?" — the slow
 *  Bash build, the fast Read, the tool that never returned. D3 (scaleLinear)
 *  maps time to the shared track; the pairing/duration math lives in the pure
 *  lib/turn-trace.ts.
 */

import { useMemo } from "react";
import { scaleLinear } from "d3-scale";
import type { WireRecord } from "@/types/wire-record";
import { buildTurnTrace, type ToolSpan } from "@/lib/turn-trace";
import { toolColor } from "@/lib/tool-colors";
import { formatDuration } from "@/lib/time";
import { cn } from "@/lib/cn";

interface Props {
  records: readonly WireRecord[];
  width?: number;
  selectedCallId?: string | null;
  onSelectSpan?: (callId: string) => void;
  className?: string;
}

export function TurnTraceView({ records, selectedCallId, onSelectSpan, className }: Props) {
  const model = useMemo(() => buildTurnTrace(records), [records]);

  // Map the time domain to a 0–100 track (percent), padding a zero-width span.
  const [d0, d1] = model.domain;
  const x = useMemo(
    () => scaleLinear().domain(d1 > d0 ? [d0, d1] : [d0, d0 + 1]).range([0, 100]),
    [d0, d1],
  );

  if (model.spans.length === 0) {
    return <div className={cn("px-3 py-4 text-[11px] text-[#565f89]", className)}>No tool calls to trace.</div>;
  }

  return (
    <div className={cn("select-none", className)}>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 pt-2 pb-1.5 text-[10px] text-[#565f89]">
        <span className="text-[#c0caf5]">{model.spans.length} tool calls</span>
        <span>· {formatDuration(model.totalMs)} span</span>
        {model.slowest && (
          <span>· slowest <span style={{ color: toolColor(model.slowest.name) }}>{model.slowest.name}</span> {formatDuration(model.slowest.durationMs ?? 0)}</span>
        )}
        {model.errorCount > 0 && <span className="text-[#f7768e]">· {model.errorCount} failed</span>}
      </div>

      <div className="max-h-[280px] overflow-y-auto pb-1">
        {model.spans.map((s) => (
          <TraceRow
            key={`${s.callId}-${s.seq}`}
            span={s}
            x={x}
            selected={selectedCallId === s.callId}
            slow={model.slowest?.callId === s.callId}
            onSelect={onSelectSpan}
          />
        ))}
      </div>
    </div>
  );
}

function TraceRow({
  span,
  x,
  selected,
  slow,
  onSelect,
}: {
  span: ToolSpan;
  x: (t: number) => number;
  selected: boolean;
  slow: boolean;
  onSelect?: (callId: string) => void;
}) {
  const left = x(span.startMs);
  const rightRaw = span.endMs !== null ? x(span.endMs) : left;
  const widthPct = Math.max(rightRaw - left, span.endMs !== null ? 0.5 : 0);
  const color = span.isError ? "#f7768e" : toolColor(span.name);
  const unresolved = span.durationMs === null;

  return (
    <button
      type="button"
      data-trace-span={span.callId}
      onClick={onSelect ? () => onSelect(span.callId) : undefined}
      className={cn(
        "flex w-full items-center gap-2 px-3 py-[3px] text-left transition-colors",
        selected ? "bg-[#283549]" : "hover:bg-[#24283b]",
      )}
      title={`${span.name}${span.detail ? " · " + span.detail : ""}${unresolved ? " · no result" : " · " + formatDuration(span.durationMs!)}`}
    >
      {/* label */}
      <span className="flex w-40 shrink-0 items-center gap-1.5 truncate">
        <span className="h-2 w-2 shrink-0 rounded-sm" style={{ background: color }} />
        <span className="truncate text-[11px] text-[#c0caf5]">{span.name}</span>
        {span.detail && <span className="truncate text-[10px] text-[#565f89]">{span.detail}</span>}
      </span>

      {/* waterfall track */}
      <span className="relative h-3 flex-1 rounded-sm bg-[#1a1b26]">
        <span
          className={cn("absolute top-0 h-3 rounded-sm", slow && "ring-1 ring-[#e0af68]")}
          style={{
            left: `${left}%`,
            width: `${widthPct}%`,
            minWidth: 3,
            background: color,
            opacity: unresolved ? 0.35 : 0.85,
            backgroundImage: unresolved
              ? "repeating-linear-gradient(45deg, transparent, transparent 3px, rgba(0,0,0,0.3) 3px, rgba(0,0,0,0.3) 6px)"
              : undefined,
          }}
        />
      </span>

      {/* duration */}
      <span className="w-14 shrink-0 text-right text-[10px] tabular-nums text-[#565f89]">
        {unresolved ? "—" : formatDuration(span.durationMs!)}
      </span>
    </button>
  );
}
