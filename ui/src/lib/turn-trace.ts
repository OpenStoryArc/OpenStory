/** Pure transform: session records → a tool-call trace with durations.
 *
 *  Observability-style: each tool_call is paired with its tool_result by
 *  call_id to form a span whose duration is the wall-clock between them. Feeds
 *  the TurnTraceView waterfall — the view that finally answers "where did the
 *  time go?" Side-effect-free so all the pairing edge cases (missing result,
 *  orphan result, out-of-order records, parallel calls) are unit-tested here.
 */

import type { WireRecord } from "@/types/wire-record";
import type { ToolCall, ToolResult } from "@/types/view-record";
import { toolInputSummary } from "@/types/view-record";

export interface ToolSpan {
  readonly callId: string;
  readonly name: string;
  readonly detail: string;
  readonly startMs: number;
  readonly endMs: number | null;
  /** null when the call has no matching result (in-flight / lost). */
  readonly durationMs: number | null;
  readonly isError: boolean;
  readonly depth: number;
  readonly agentId: string | null;
  readonly seq: number;
}

export interface TraceModel {
  readonly spans: ToolSpan[];
  /** [minStart, maxEnd] in epoch ms; [0,0] when empty. */
  readonly domain: [number, number];
  readonly totalMs: number;
  readonly slowest: ToolSpan | null;
  readonly errorCount: number;
  /** Max nesting depth + 1 (for subagent lanes). */
  readonly laneCount: number;
}

export function buildTurnTrace(records: readonly WireRecord[]): TraceModel {
  // First pass: index results by call_id (first result wins).
  const results = new Map<string, WireRecord>();
  for (const r of records) {
    if (r.record_type !== "tool_result") continue;
    const callId = (r.payload as ToolResult)?.call_id;
    if (callId && !results.has(callId)) results.set(callId, r);
  }

  const spans: ToolSpan[] = [];
  let errorCount = 0;

  for (const r of records) {
    if (r.record_type !== "tool_call") continue;
    const call = r.payload as ToolCall;
    const callId = call?.call_id;
    if (!callId) continue;

    const startMs = Date.parse(r.timestamp);
    if (!Number.isFinite(startMs)) continue;

    const result = results.get(callId);
    const endRaw = result ? Date.parse(result.timestamp) : NaN;
    const endMs = Number.isFinite(endRaw) ? endRaw : null;
    // Guard against a result timestamped before its call (clock skew).
    const durationMs = endMs !== null ? Math.max(0, endMs - startMs) : null;
    const isError = Boolean(result && (result.payload as ToolResult)?.is_error);
    if (isError) errorCount += 1;

    spans.push({
      callId,
      name: call.name || "tool",
      detail: toolInputSummary(call.typed_input) || "",
      startMs,
      endMs,
      durationMs,
      isError,
      depth: r.depth ?? 0,
      agentId: r.agent_id ?? null,
      seq: r.seq,
    });
  }

  spans.sort((a, b) => a.startMs - b.startMs || a.seq - b.seq);

  const starts = spans.map((s) => s.startMs);
  const ends = spans.map((s) => s.endMs ?? s.startMs);
  const start = starts.length ? Math.min(...starts) : 0;
  const end = ends.length ? Math.max(...ends) : 0;

  let slowest: ToolSpan | null = null;
  for (const s of spans) {
    if (s.durationMs === null) continue;
    if (!slowest || s.durationMs > (slowest.durationMs ?? -1)) slowest = s;
  }

  const laneCount = spans.reduce((m, s) => Math.max(m, s.depth + 1), 0);

  return {
    spans,
    domain: [start, end],
    totalMs: end - start,
    slowest,
    errorCount,
    laneCount,
  };
}
