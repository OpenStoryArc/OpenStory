// types.ts — The shapes of data flowing through the system.
//
// Three layers:
//   1. API types — what OpenStory returns
//   2. Detector types — internal state machine
//   3. Structural types — what the detector produces
//
// No logic here. Just shapes. Tagged unions all the way down.

// ─────────────────────────────────────────────
// Layer 1: OpenStory API response types
// ─────────────────────────────────────────────

export type RecordType =
  | "user_message"
  | "assistant_message"
  | "tool_call"
  | "tool_result"
  | "turn_end"
  | "token_usage"
  | "system_event"
  | "reasoning"
  | "file_snapshot";

export interface ApiRecord {
  id: string;
  record_type: RecordType;
  seq: number;
  session_id: string;
  timestamp: string;
  depth: number;
  parent_uuid: string | null;
  is_sidechain: boolean;
  payload: Record<string, any>;
  payload_bytes: number;
  truncated: boolean;
}

export interface ApiSession {
  session_id: string;
  project_name: string;
  label: string;
  first_prompt: string;
  model: string;
  status: string;
  event_count: number;
  tool_calls: number;
  start_time: string;
  duration_ms: number | null;
  total_input_tokens: number;
  total_output_tokens: number;
}

// ─────────────────────────────────────────────
// Layer 2: Detector state (internal)
// ─────────────────────────────────────────────

export type DetectorPhase = "idle" | "saw_eval" | "saw_apply" | "results_ready";

export interface DetectorState {
  phase: DetectorPhase;
  turnNumber: number;
  scopeDepth: number;
  envSize: number;
  pendingTools: string[];
  currentTurnIds: string[];
  currentTurnStart: string;
  scopeStack: number[];
  lastDepth: number;
}

export function initialState(): DetectorState {
  return {
    phase: "idle",
    turnNumber: 0,
    scopeDepth: 0,
    envSize: 0,
    pendingTools: [],
    currentTurnIds: [],
    currentTurnStart: "",
    scopeStack: [],
    lastDepth: 0,
  };
}

// ─────────────────────────────────────────────
// Layer 3: Structural events (output)
// ─────────────────────────────────────────────
//
// These are what the detector produces.
// Each one annotates a moment in the session
// with its computational meaning.

export type StructuralPhase =
  | "human"         // user message — the input to eval
  | "thinking"      // reasoning block — the model working through it
  | "eval"          // model examined environment, produced expression
  | "apply"         // tool dispatched (operator + operand → value)
  | "turn_end"      // one step of the coalgebra complete
  | "scope_open"    // compound procedure: nested eval-apply begins
  | "scope_close"   // nested scope returns
  | "compact";      // GC: context compaction

export interface StructuralEvent {
  phase: StructuralPhase;
  timestamp: string;
  turnNumber: number;
  scopeDepth: number;
  envSize: number;
  eventIds: string[];
  toolName?: string;
  toolInput?: string;
  toolOutput?: string;
  stopReason?: string;
  content?: string;         // truncated content for display
  tokens?: number;          // token count if available
  summary: string;          // human-readable description
}

// ─────────────────────────────────────────────
// Aggregated session view
// ─────────────────────────────────────────────

// What the eval decided to do
export type EvalDecision = "text_only" | "tool_use" | "text_and_tool_use";

export interface StructuralTurn {
  turnNumber: number;
  scopeDepth: number;
  human: {
    content: string;
    timestamp: string;
  } | null;
  thinking: {
    summary: string;
    tokens: number;
  } | null;
  eval: {
    content: string;
    timestamp: string;
    decision: EvalDecision;
    tokens: number;
  } | null;
  applies: Array<{
    toolName: string;
    inputSummary: string;
    outputSummary: string;
    isAgent: boolean;
    isError: boolean;
  }>;
  envSize: number;
  envDelta: number;         // messages added this turn
  stopReason: string;
  isTerminal: boolean;
  timestamp: string;
  durationMs: number | null;
}

export interface SessionSummary {
  sessionId: string;
  label: string;
  model: string;
  turns: StructuralTurn[];
  totalEvals: number;
  totalApplies: number;
  totalThinkingTokens: number;
  maxScopeDepth: number;
  compactionCount: number;
  toolCounts: Record<string, number>;
  envGrowth: [number, number];
}
