import type { ViewRecord } from "./view-record";
import type { WireRecord } from "./wire-record";
import type { SessionSummary } from "./session";

/** Discriminated union for all WebSocket messages from the server.
 *
 *  The server is the BFF: it transforms raw CloudEvents into typed
 *  ViewRecords before broadcasting. The UI receives pre-typed data. */
export type WsMessage = InitialStateMessage | EnrichedInitialStateMessage | SessionListMessage | ViewRecordsMessage | EnrichedMessage | PlanSavedMessage;

/** @deprecated Phase 2 initial_state (flat ViewRecords). Use EnrichedInitialStateMessage. */
export interface InitialStateMessage {
  readonly kind: "initial_state";
  readonly view_records: readonly ViewRecord[];
}

/** Session label data from server. */
export interface SessionLabel {
  readonly label: string | null;
  readonly branch: string | null;
  readonly total_input_tokens?: number;
  readonly total_output_tokens?: number;
}

/** Phase 3 initial_state: WireRecords + per-session filter counts + patterns + labels. */
export interface EnrichedInitialStateMessage {
  readonly kind: "initial_state";
  readonly records: readonly WireRecord[];
  readonly filter_counts: Readonly<Record<string, Record<string, number>>>;
  readonly patterns?: readonly ServerPatternEvent[];
  readonly session_labels?: Readonly<Record<string, SessionLabel>>;
}

/** Phase 3 enriched broadcast: durable + ephemeral + filter deltas + patterns + labels. */
export interface EnrichedMessage {
  readonly kind: "enriched";
  readonly session_id: string;
  readonly records: readonly WireRecord[];
  readonly ephemeral: readonly ViewRecord[];
  readonly filter_deltas: Readonly<Record<string, number>>;
  readonly patterns?: readonly ServerPatternEvent[];
  readonly project_id?: string;
  readonly project_name?: string;
  readonly session_label?: string;
  readonly session_branch?: string;
  readonly total_input_tokens?: number;
  readonly total_output_tokens?: number;
}

/** Server-side PatternEvent shape (as serialized by Rust). */
export interface ServerPatternEvent {
  readonly pattern_type: string;
  readonly session_id: string;
  readonly event_ids: readonly string[];
  readonly started_at: string;
  readonly ended_at: string;
  readonly summary: string;
  readonly metadata: Record<string, unknown>;
}

/** @deprecated Server now sends initial_state instead. Kept for backwards compat. */
export interface SessionListMessage {
  readonly kind: "session_list";
  readonly sessions: readonly SessionSummary[];
}

export interface ViewRecordsMessage {
  readonly kind: "view_records";
  readonly session_id: string;
  readonly view_records: readonly ViewRecord[];
  readonly project_id?: string;
  readonly project_name?: string;
}

export interface PlanSavedMessage {
  readonly kind: "plan_saved";
  readonly session_id: string;
}
