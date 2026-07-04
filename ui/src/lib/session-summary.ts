/** Pure fold: session records → a compact at-a-glance summary.
 *
 *  The one model behind the shared SessionSummary header that appears across
 *  Explore / Overview / Story, so the app reads as one product: how long, how
 *  many tools, how many errors, how many tokens, which files, which model.
 *  Side-effect-free — a straight fold over the records, unit-tested here.
 */

import type { WireRecord } from "@/types/wire-record";
import type { AssistantMessage, SessionMeta, ToolCall, ToolResult, TokenUsage } from "@/types/view-record";

/** Id of the earliest failure (error record or errored tool_result), by seq.
 *  Null when the session had no failures. Used to jump-to-first-error. */
export function firstErrorEventId(records: readonly WireRecord[]): string | null {
  let best: WireRecord | null = null;
  for (const r of records) {
    const isError = r.record_type === "error" || (r.record_type === "tool_result" && (r.payload as ToolResult)?.is_error);
    if (!isError) continue;
    if (!best || r.seq < best.seq) best = r;
  }
  return best?.id ?? null;
}

export interface FileTouch {
  readonly path: string;
  readonly count: number;
}

export interface SessionSummary {
  readonly eventCount: number;
  readonly toolCount: number;
  readonly errorCount: number;
  readonly turnCount: number;
  readonly startMs: number | null;
  readonly endMs: number | null;
  readonly durationMs: number;
  readonly inputTokens: number;
  readonly outputTokens: number;
  readonly cacheCreationTokens: number;
  readonly cacheReadTokens: number;
  /** All categories summed — input + output + cache write + cache read. */
  readonly totalTokens: number;
  readonly model: string | null;
  readonly topFiles: FileTouch[];
}

/** File path from a file-touching tool input, else null. */
function filePathOf(call: ToolCall): string | null {
  const ti = call.typed_input;
  if (!ti) return null;
  switch (ti.tool) {
    case "read":
    case "edit":
    case "write":
      return ti.file_path;
    case "notebook_edit":
      return ti.notebook_path;
    default:
      return null;
  }
}

/** Shape of GET /api/sessions/{id}/summary. Extras (turn_count, tokens,
 *  top_files) are present when the server serves from its projection;
 *  absent on the fallback path — the mapper degrades to zeros. */
export interface ApiSessionSummary {
  readonly session_id: string;
  readonly status: string;
  readonly start_time?: string | null;
  readonly last_event?: string | null;
  readonly duration_ms?: number | null;
  readonly event_count: number;
  readonly error_count: number;
  readonly tool_calls: number;
  readonly model?: string | null;
  readonly turn_count?: number;
  readonly tokens?: {
    readonly input: number;
    readonly output: number;
    readonly cache_creation: number;
    readonly cache_read: number;
    readonly total: number;
  };
  readonly top_files?: readonly { path: string; count: number }[];
}

/** Map the /summary payload into the same SessionSummary the records fold
 *  produces, so one header component serves both sources. */
export function summaryFromApi(api: ApiSessionSummary): SessionSummary {
  const startMs = api.start_time ? Date.parse(api.start_time) : NaN;
  const endMs = api.last_event ? Date.parse(api.last_event) : NaN;
  const spanMs =
    Number.isFinite(startMs) && Number.isFinite(endMs) ? endMs - startMs : 0;
  const t = api.tokens;
  return {
    eventCount: api.event_count,
    toolCount: api.tool_calls,
    errorCount: api.error_count,
    turnCount: api.turn_count ?? 0,
    startMs: Number.isFinite(startMs) ? startMs : null,
    endMs: Number.isFinite(endMs) ? endMs : null,
    durationMs: api.duration_ms ?? spanMs,
    inputTokens: t?.input ?? 0,
    outputTokens: t?.output ?? 0,
    cacheCreationTokens: t?.cache_creation ?? 0,
    cacheReadTokens: t?.cache_read ?? 0,
    totalTokens: t?.total ?? 0,
    model: api.model ?? null,
    topFiles: (api.top_files ?? []).map(({ path, count }) => ({ path, count })),
  };
}

export function buildSessionSummary(records: readonly WireRecord[]): SessionSummary {
  let toolCount = 0;
  let errorCount = 0;
  let turnCount = 0;
  let inputTokens = 0;
  let outputTokens = 0;
  let cacheCreationTokens = 0;
  let cacheReadTokens = 0;
  let model: string | null = null;
  let minMs = Infinity;
  let maxMs = -Infinity;
  const files = new Map<string, number>();

  for (const r of records) {
    const t = Date.parse(r.timestamp);
    if (Number.isFinite(t)) {
      if (t < minMs) minMs = t;
      if (t > maxMs) maxMs = t;
    }

    switch (r.record_type) {
      case "tool_call": {
        toolCount += 1;
        const path = filePathOf(r.payload as ToolCall);
        if (path) files.set(path, (files.get(path) ?? 0) + 1);
        break;
      }
      case "tool_result":
        if ((r.payload as ToolResult)?.is_error) errorCount += 1;
        break;
      case "error":
        errorCount += 1;
        break;
      case "turn_end":
        turnCount += 1;
        break;
      case "token_usage": {
        const u = r.payload as TokenUsage;
        if (u?.scope === "turn") {
          inputTokens += u.input_tokens ?? 0;
          outputTokens += u.output_tokens ?? 0;
          cacheCreationTokens += u.cache_creation_input_tokens ?? 0;
          cacheReadTokens += u.cache_read_input_tokens ?? 0;
        }
        break;
      }
      case "assistant_message":
        if (!model) model = (r.payload as AssistantMessage)?.model ?? null;
        break;
      case "session_meta":
        if (!model) model = (r.payload as SessionMeta)?.model ?? null;
        break;
    }
  }

  const topFiles = [...files.entries()]
    .map(([path, count]) => ({ path, count }))
    .sort((a, b) => b.count - a.count || a.path.localeCompare(b.path))
    .slice(0, 5);

  const startMs = Number.isFinite(minMs) ? minMs : null;
  const endMs = Number.isFinite(maxMs) ? maxMs : null;

  return {
    eventCount: records.length,
    toolCount,
    errorCount,
    turnCount,
    startMs,
    endMs,
    durationMs: startMs !== null && endMs !== null ? endMs - startMs : 0,
    inputTokens,
    outputTokens,
    cacheCreationTokens,
    cacheReadTokens,
    totalTokens: inputTokens + outputTokens + cacheCreationTokens + cacheReadTokens,
    model,
    topFiles,
  };
}
