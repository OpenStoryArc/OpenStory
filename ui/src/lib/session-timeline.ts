/** Pure transform: session records → a temporal "activity ribbon" model.
 *
 *  This is the data model behind the SessionActivityRibbon viz. It is a pure
 *  function (records in, model out) so it can be unit-tested exhaustively and
 *  reused headlessly. All rendering/D3 concerns live at the component boundary.
 *
 *  Records are laid out on swimlanes by kind (user / reasoning / assistant /
 *  tool / system) and positioned in time. token_usage records don't get a mark
 *  — instead they feed a cumulative output-token "burn" series drawn underneath.
 */

import type { WireRecord } from "@/types/wire-record";
import type { RecordType, ToolCall, ToolResult, TokenUsage } from "@/types/view-record";
import { toolColor } from "@/lib/tool-colors";

export type LaneKey = "user" | "reasoning" | "assistant" | "tool" | "system";

/** Canonical top-to-bottom lane order. `model.lanes` is a subset of this. */
export const LANE_ORDER: readonly LaneKey[] = ["user", "reasoning", "assistant", "tool", "system"];

/** Lane base colors (Tokyonight). Tool marks override with per-tool colors. */
export const LANE_COLOR: Record<LaneKey, string> = {
  user: "#9ece6a",
  reasoning: "#bb9af7",
  assistant: "#7aa2f7",
  tool: "#7dcfff",
  system: "#565f89",
};

const ERROR_COLOR = "#f7768e";

export interface RibbonEvent {
  readonly id: string;
  readonly seq: number;
  /** Epoch milliseconds. */
  readonly t: number;
  readonly lane: LaneKey;
  readonly recordType: RecordType;
  /** Short human label (tool name, or humanized record type). */
  readonly label: string;
  readonly color: string;
  readonly isError: boolean;
  /** Payload size in bytes — used to scale mark radius. */
  readonly bytes: number;
}

export interface TokenPoint {
  readonly t: number;
  readonly cumulative: number;
}

export interface TimelineModel {
  /** [startMs, endMs]. [0, 0] when empty. */
  readonly domain: [number, number];
  readonly durationMs: number;
  /** Lanes actually present, in LANE_ORDER. */
  readonly lanes: LaneKey[];
  /** Marks, sorted by time then seq. */
  readonly events: RibbonEvent[];
  /** Cumulative output-token burn across turn-scoped usage. */
  readonly tokenSeries: TokenPoint[];
  readonly totalTokens: number;
  readonly laneCounts: Record<LaneKey, number>;
  readonly errorCount: number;
}

/** Which swimlane a record type belongs to. `null` = not drawn as a mark. */
export function laneFor(type: RecordType): LaneKey | null {
  switch (type) {
    case "user_message":
      return "user";
    case "reasoning":
      return "reasoning";
    case "assistant_message":
      return "assistant";
    case "tool_call":
    case "tool_result":
      return "tool";
    case "token_usage":
      return null;
    default:
      // turn_start, turn_end, session_meta, file_snapshot,
      // context_compaction, system_event, error
      return "system";
  }
}

function humanize(type: RecordType): string {
  return type.replace(/_/g, " ");
}

function markLabel(record: WireRecord): string {
  if (record.record_type === "tool_call") {
    const name = (record.payload as ToolCall)?.name;
    return name || "tool";
  }
  if (record.record_type === "tool_result") return "result";
  return humanize(record.record_type);
}

function isErrorRecord(record: WireRecord): boolean {
  if (record.record_type === "error") return true;
  if (record.record_type === "tool_result") return Boolean((record.payload as ToolResult)?.is_error);
  return false;
}

function markColor(record: WireRecord, lane: LaneKey, isError: boolean): string {
  if (isError) return ERROR_COLOR;
  if (record.record_type === "tool_call") {
    const name = (record.payload as ToolCall)?.name;
    return toolColor(name || "Other");
  }
  return LANE_COLOR[lane];
}

export function buildTimelineModel(records: readonly WireRecord[]): TimelineModel {
  const events: RibbonEvent[] = [];
  const tokenSeries: TokenPoint[] = [];
  const laneCounts: Record<LaneKey, number> = { user: 0, reasoning: 0, assistant: 0, tool: 0, system: 0 };
  let cumulative = 0;
  let errorCount = 0;

  // Preserve stream order for token accumulation.
  const ordered = [...records].sort((a, b) => a.seq - b.seq);

  for (const record of ordered) {
    const t = Date.parse(record.timestamp);
    if (!Number.isFinite(t)) continue;

    if (record.record_type === "token_usage") {
      const usage = record.payload as TokenUsage;
      if (usage?.scope === "turn") {
        cumulative += usage.output_tokens ?? 0;
        tokenSeries.push({ t, cumulative });
      }
      continue;
    }

    const lane = laneFor(record.record_type);
    if (!lane) continue;

    const isError = isErrorRecord(record);
    if (isError) errorCount += 1;
    laneCounts[lane] += 1;

    events.push({
      id: record.id,
      seq: record.seq,
      t,
      lane,
      recordType: record.record_type,
      label: markLabel(record),
      color: markColor(record, lane, isError),
      isError,
      bytes: record.payload_bytes ?? 0,
    });
  }

  events.sort((a, b) => a.t - b.t || a.seq - b.seq);

  const times = events.map((e) => e.t).concat(tokenSeries.map((p) => p.t));
  const start = times.length ? Math.min(...times) : 0;
  const end = times.length ? Math.max(...times) : 0;

  const lanes = LANE_ORDER.filter((l) => laneCounts[l] > 0);

  return {
    domain: [start, end],
    durationMs: end - start,
    lanes: [...lanes],
    events,
    tokenSeries,
    totalTokens: cumulative,
    laneCounts,
    errorCount,
  };
}
