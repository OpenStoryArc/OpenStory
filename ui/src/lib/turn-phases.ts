/**
 * Derive turn phase segments from a session's records.
 *
 * Replaces the legacy `turn.phase` pattern type. The persisted pattern was
 * a redundant projection of data already in the record stream — this module
 * computes the same projection on the fly. Algorithm ported from the legacy
 * `TurnPhaseDetector` (rs/patterns/src/turn_phase.rs):
 *
 *   1. Walk records sequentially.
 *   2. Each `user_message` record marks a new turn boundary.
 *   3. Within a turn, count tool_call records by tool name.
 *   4. Apply classification rules to produce one of:
 *        conversation, exploration, implementation, implementation+testing,
 *        testing, execution, delegation, mixed.
 *
 * The classifier is a pure function — `classifyPhase(toolCounts)` — that takes
 * a map of tool name → call count and returns the phase string.
 */
import type { WireRecord } from "@/types/wire-record";

export interface TurnPhaseSegment {
  readonly phase: string;
  readonly eventCount: number;
  readonly events: readonly string[];
}

const EXPLORE_TOOLS = new Set(["Read", "Grep", "Glob"]);
const EXPLORE_AND_BASH = new Set(["Read", "Grep", "Glob", "Bash"]);

/** Pure: count map of tool calls → phase label. */
export function classifyPhase(tools: ReadonlyMap<string, number>): string {
  if (tools.size === 0) {
    return "conversation";
  }

  const toolNames = new Set(tools.keys());

  // exploration: only Read/Grep/Glob (+optional Bash where explore tools dominate)
  const allInExploreSet = [...toolNames].every((t) => EXPLORE_AND_BASH.has(t));
  if (allInExploreSet) {
    let exploreCount = 0;
    for (const t of EXPLORE_TOOLS) {
      exploreCount += tools.get(t) ?? 0;
    }
    const bashCount = tools.get("Bash") ?? 0;
    if (exploreCount > bashCount) {
      return "exploration";
    }
  }

  // implementation: Edit/Write present
  if (toolNames.has("Edit") || toolNames.has("Write")) {
    const editCount = (tools.get("Edit") ?? 0) + (tools.get("Write") ?? 0);
    const bashCount = tools.get("Bash") ?? 0;
    if (bashCount > editCount) {
      return "implementation+testing";
    }
    return "implementation";
  }

  // delegation: subagent invocation
  if (toolNames.has("Task")) {
    return "delegation";
  }

  // bash-only turns: execution vs testing
  if (toolNames.has("Bash")) {
    const bashCount = tools.get("Bash") ?? 0;
    if (bashCount > 5) {
      return "testing";
    }
    return "execution";
  }

  return "mixed";
}

/** Pure: extract the tool name from a tool_call WireRecord, or null. */
function toolNameOf(record: WireRecord): string | null {
  if (record.record_type !== "tool_call") {
    return null;
  }
  // ToolCall payload has `name: string` — see ui/src/types/view-record.ts
  const payload = record.payload as { name?: unknown };
  return typeof payload.name === "string" ? payload.name : null;
}

/**
 * Walk a session's records and produce one phase segment per turn.
 * A turn = the records from one `user_message` boundary to the next
 * (or to the end of the stream).
 */
export function extractTurnPhases(records: readonly WireRecord[]): TurnPhaseSegment[] {
  if (records.length === 0) {
    return [];
  }

  const segments: TurnPhaseSegment[] = [];
  let currentTurn: WireRecord[] = [];

  const flush = () => {
    if (currentTurn.length === 0) return;
    const tools = new Map<string, number>();
    for (const r of currentTurn) {
      const name = toolNameOf(r);
      if (name) {
        tools.set(name, (tools.get(name) ?? 0) + 1);
      }
    }
    segments.push({
      phase: classifyPhase(tools),
      eventCount: currentTurn.length,
      events: currentTurn.map((r) => r.id),
    });
    currentTurn = [];
  };

  for (const record of records) {
    if (record.record_type === "user_message" && currentTurn.length > 0) {
      flush();
    }
    currentTurn.push(record);
  }
  // Tail turn
  flush();

  return segments;
}
