/**
 * Synthetic event generator for performance and chaos testing.
 *
 * Pure functions: config in, events out. Deterministic with seed.
 * Produces structurally valid WireRecords that pass through the
 * real UI pipeline (enrichedReducer → toTimelineRows → render).
 *
 * This is the foundation of all battle-hardening tests.
 */

import type { WireRecord } from "@/types/wire-record";
import type { RecordType, ToolCall, ToolResult, UserMessage, AssistantMessage, Reasoning, ErrorRecord, SystemEvent, TurnEnd } from "@/types/view-record";
import type { InitialStateMessage, EnrichedMessage } from "@/types/websocket";

// ═══════════════════════════════════════════════════════════════════
// Seeded PRNG — deterministic, fast, good enough for test data
// ═══════════════════════════════════════════════════════════════════

/** Mulberry32: 32-bit seeded PRNG. Returns [0, 1). */
function mulberry32(seed: number): () => number {
  let s = seed | 0;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ═══════════════════════════════════════════════════════════════════
// Config types
// ═══════════════════════════════════════════════════════════════════

export interface SynthConfig {
  /** Number of records to generate. */
  count: number;
  /** Number of concurrent sessions. Default 1. */
  sessions?: number;
  /** Deterministic seed. Default: Date.now(). */
  seed?: number;
  /** Weighted record type distribution. Default: realistic mix. */
  typeWeights?: Partial<Record<RecordType, number>>;
}

export interface SynthStreamConfig {
  /** Number of enriched message batches. */
  batches: number;
  /** Records per batch. */
  recordsPerBatch: number;
  /** Number of sessions. Default 1. */
  sessions?: number;
  /** Deterministic seed. Default: Date.now(). */
  seed?: number;
}

// ═══════════════════════════════════════════════════════════════════
// Default weights — realistic agent session distribution
// ═══════════════════════════════════════════════════════════════════

const DEFAULT_WEIGHTS: Record<RecordType, number> = {
  tool_call: 30,
  tool_result: 30,
  assistant_message: 15,
  user_message: 5,
  reasoning: 10,
  turn_end: 3,
  system_event: 3,
  error: 2,
  session_meta: 0,
  turn_start: 0,
  token_usage: 0,
  context_compaction: 0,
  file_snapshot: 2,
};

// ═══════════════════════════════════════════════════════════════════
// Tool names with weights (realistic distribution)
// ═══════════════════════════════════════════════════════════════════

const TOOL_NAMES = [
  { name: "Bash", weight: 30 },
  { name: "Read", weight: 25 },
  { name: "Edit", weight: 15 },
  { name: "Write", weight: 8 },
  { name: "Grep", weight: 10 },
  { name: "Glob", weight: 5 },
  { name: "Agent", weight: 5 },
  { name: "WebSearch", weight: 2 },
];

const TOOL_TOTAL_WEIGHT = TOOL_NAMES.reduce((s, t) => s + t.weight, 0);

// ═══════════════════════════════════════════════════════════════════
// Weighted random selection
// ═══════════════════════════════════════════════════════════════════

function weightedPick<T>(items: { value: T; weight: number }[], rand: () => number): T {
  const total = items.reduce((s, i) => s + i.weight, 0);
  let r = rand() * total;
  for (const item of items) {
    r -= item.weight;
    if (r <= 0) return item.value;
  }
  return items[items.length - 1]!.value;
}

function pickRecordType(
  weights: Record<RecordType, number>,
  rand: () => number,
): RecordType {
  const items = Object.entries(weights)
    .filter(([, w]) => w > 0)
    .map(([k, w]) => ({ value: k as RecordType, weight: w }));
  return weightedPick(items, rand);
}

function pickToolName(rand: () => number): string {
  let r = rand() * TOOL_TOTAL_WEIGHT;
  for (const t of TOOL_NAMES) {
    r -= t.weight;
    if (r <= 0) return t.name;
  }
  return "Bash";
}

// ═══════════════════════════════════════════════════════════════════
// ID generation
// ═══════════════════════════════════════════════════════════════════

function makeId(rand: () => number): string {
  const hex = () => Math.floor(rand() * 0x10000).toString(16).padStart(4, "0");
  return `${hex()}${hex()}-${hex()}-${hex()}-${hex()}-${hex()}${hex()}${hex()}`;
}

// ═══════════════════════════════════════════════════════════════════
// Payload factories
// ═══════════════════════════════════════════════════════════════════

function makePayload(recordType: RecordType, rand: () => number): unknown {
  switch (recordType) {
    case "tool_call": {
      const name = pickToolName(rand);
      const callId = makeId(rand);
      const payload: ToolCall = {
        call_id: callId,
        name,
        input: { command: `echo test-${Math.floor(rand() * 10000)}` },
        raw_input: { command: `echo test-${Math.floor(rand() * 10000)}` },
        typed_input: { tool: "bash", command: `echo test-${Math.floor(rand() * 10000)}` },
      };
      return payload;
    }
    case "tool_result": {
      const payload: ToolResult = {
        call_id: makeId(rand),
        output: `Output line ${Math.floor(rand() * 10000)}\nMore output here.`,
        is_error: rand() < 0.05,
      };
      return payload;
    }
    case "user_message": {
      const payload: UserMessage = {
        content: `User message content ${Math.floor(rand() * 10000)}`,
      };
      return payload;
    }
    case "assistant_message": {
      const payload: AssistantMessage = {
        model: "claude-opus-4-6",
        content: [{ type: "text", text: `Assistant response ${Math.floor(rand() * 10000)}` }],
        stop_reason: "end_turn",
      };
      return payload;
    }
    case "reasoning": {
      const payload: Reasoning = {
        summary: [`Thinking about step ${Math.floor(rand() * 100)}`],
        content: `Extended reasoning content ${Math.floor(rand() * 10000)}`,
        encrypted: false,
      };
      return payload;
    }
    case "error": {
      const payload: ErrorRecord = {
        code: "ERR_TEST",
        message: `Test error ${Math.floor(rand() * 1000)}`,
        details: "Synthetic error for testing",
      };
      return payload;
    }
    case "system_event": {
      const payload: SystemEvent = {
        subtype: "system.test",
        message: `System event ${Math.floor(rand() * 1000)}`,
      };
      return payload;
    }
    case "turn_end": {
      const payload: TurnEnd = {
        reason: "end_turn",
        duration_ms: Math.floor(rand() * 30000),
      };
      return payload;
    }
    default:
      return { subtype: recordType };
  }
}

// ═══════════════════════════════════════════════════════════════════
// Single record factory
// ═══════════════════════════════════════════════════════════════════

let globalSeq = 0;
let globalRand = mulberry32(Date.now());

/** Generate a single synthetic WireRecord. Accepts field overrides. */
export function synth(
  overrides?: Partial<WireRecord> & { record_type?: RecordType },
): WireRecord {
  const rand = globalRand;
  const recordType = overrides?.record_type ?? pickRecordType(DEFAULT_WEIGHTS, rand);
  const id = overrides?.id ?? makeId(rand);
  const seq = overrides?.seq ?? globalSeq++;
  const sessionId = overrides?.session_id ?? `synth-session-${Math.floor(rand() * 1000)}`;
  const now = new Date(Date.now() - Math.floor(rand() * 3600000));
  const timestamp = overrides?.timestamp ?? now.toISOString();
  const payload = overrides?.payload ?? makePayload(recordType, rand);
  const depth = overrides?.depth ?? 0;
  const parentUuid = overrides?.parent_uuid ?? null;

  const payloadStr = JSON.stringify(payload);

  return {
    id,
    seq,
    session_id: sessionId,
    timestamp,
    record_type: recordType,
    payload: payload as WireRecord["payload"],
    agent_id: overrides?.agent_id ?? null,
    is_sidechain: overrides?.is_sidechain ?? false,
    depth,
    parent_uuid: parentUuid,
    truncated: overrides?.truncated ?? false,
    payload_bytes: overrides?.payload_bytes ?? payloadStr.length,
  };
}

// ═══════════════════════════════════════════════════════════════════
// Batch factory — deterministic, structurally valid
// ═══════════════════════════════════════════════════════════════════

/** Generate a batch of structurally valid WireRecords. */
export function synthBatch(config: SynthConfig): WireRecord[] {
  const { count, sessions = 1, seed = 1 } = config;
  const rand = mulberry32(seed);
  const weights = { ...DEFAULT_WEIGHTS, ...config.typeWeights };

  // Normalize weights — if custom weights provided, zero out defaults not mentioned
  if (config.typeWeights) {
    for (const key of Object.keys(DEFAULT_WEIGHTS) as RecordType[]) {
      if (!(key in config.typeWeights)) {
        weights[key] = 0;
      }
    }
    // Ensure at least one type has weight
    const totalWeight = Object.values(weights).reduce((s, w) => s + w, 0);
    if (totalWeight === 0) weights.tool_call = 1;
  }

  // Generate session IDs
  const sessionIds = Array.from({ length: sessions }, (_, i) =>
    `synth-${seed}-s${i}`,
  );

  // Per-session state
  const sessionState = new Map<string, { seq: number; ids: string[]; baseTime: number }>();
  for (const sid of sessionIds) {
    sessionState.set(sid, {
      seq: 0,
      ids: [],
      baseTime: Date.now() - 3600000 + Math.floor(rand() * 1000),
    });
  }

  const records: WireRecord[] = [];
  const depthIndex = new Map<string, number>();

  for (let i = 0; i < count; i++) {
    // Round-robin sessions
    const sid = sessionIds[i % sessions]!;
    const state = sessionState.get(sid)!;

    const recordType = pickRecordType(weights, rand);
    const id = makeId(rand);
    const seq = state.seq++;
    const timestamp = new Date(state.baseTime + seq * 50 + Math.floor(rand() * 20)).toISOString();
    const payload = makePayload(recordType, rand);

    // Depth: mostly 0, occasionally deeper
    let depth = 0;
    let parentUuid: string | null = null;
    if (state.ids.length > 0 && rand() < 0.2) {
      // 20% chance of being a child of a recent record
      const parentIdx = Math.max(0, state.ids.length - 1 - Math.floor(rand() * 5));
      parentUuid = state.ids[parentIdx]!;
      // Look up parent depth from our index (O(1) vs O(n) scan)
      const parentDepth = depthIndex.get(parentUuid) ?? 0;
      depth = rand() < 0.5 ? parentDepth : parentDepth + 1;
    }

    state.ids.push(id);
    depthIndex.set(id, depth);

    const payloadStr = JSON.stringify(payload);
    records.push({
      id,
      seq,
      session_id: sid,
      timestamp,
      record_type: recordType,
      payload: payload as WireRecord["payload"],
      agent_id: null,
      is_sidechain: false,
      depth,
      parent_uuid: parentUuid,
      truncated: false,
      payload_bytes: payloadStr.length,
    });
  }

  return records;
}

// ═══════════════════════════════════════════════════════════════════
// WsMessage factories
// ═══════════════════════════════════════════════════════════════════

/** Generate the new sidebar-only `initial_state` message. After the
 *  lazy-load redesign (feat/lazy-load-initial-state) this no longer
 *  carries records — those arrive per-session via REST. Tests that
 *  need to seed records into reducer state should use
 *  `synthBatch(...)` and dispatch a `session_records_loaded` action
 *  directly. The `_config` arg is retained for call-site compatibility
 *  with the pre-redesign signature; it has no effect on the message. */
export function synthInitialState(_config: SynthConfig = { count: 0 }): InitialStateMessage {
  return {
    kind: "initial_state",
    patterns: [],
    session_labels: {},
  };
}

/** Generate a sequence of enriched WsMessages for streaming tests.
 *  The wire `filter_deltas` field is still part of the protocol (the
 *  server emits it) but the UI reducer ignores it after the redesign. */
export function synthEnrichedStream(config: SynthStreamConfig): EnrichedMessage[] {
  const { batches, recordsPerBatch, sessions = 1, seed = 1 } = config;
  const totalRecords = batches * recordsPerBatch;
  const allRecords = synthBatch({ count: totalRecords, sessions, seed });

  const sessionIds = [...new Set(allRecords.map((r) => r.session_id))];
  const messages: EnrichedMessage[] = [];

  for (let b = 0; b < batches; b++) {
    const batchRecords = allRecords.slice(
      b * recordsPerBatch,
      (b + 1) * recordsPerBatch,
    );
    const sid = sessionIds[b % sessionIds.length]!;
    messages.push({
      kind: "enriched",
      session_id: sid,
      records: batchRecords,
      ephemeral: [],
      filter_deltas: {},
    });
  }

  return messages;
}
