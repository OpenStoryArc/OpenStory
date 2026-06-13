import type { ViewRecord } from "./view-record";
import type { WireRecord } from "./wire-record";
import type { SessionSummary } from "./session";

/** Discriminated union for all WebSocket messages from the server.
 *
 *  The server is the BFF: it transforms raw CloudEvents into typed
 *  ViewRecords before broadcasting. The UI receives pre-typed data.
 *
 *  After feat/lazy-load-initial-state, `initial_state` carries only
 *  session_labels and recent patterns — records are fetched per-session
 *  via REST. The legacy `view_records` shape and the records-bearing
 *  `EnrichedInitialStateMessage` are gone. */
export type WsMessage = InitialStateMessage | SessionListMessage | ViewRecordsMessage | EnrichedMessage | PlanSavedMessage | ShapesMessage;

/** Session label data from server. */
export interface SessionLabel {
  readonly label: string | null;
  readonly branch: string | null;
  readonly total_input_tokens?: number;
  readonly total_output_tokens?: number;
}

/** Sidebar-only handshake. No records — the UI lazy-loads them from
 *  GET /api/sessions/{id}/records when the user opens a session.
 *  Bounded by `config.watch_backfill_hours` on the server, so even a
 *  store with thousands of historical sessions stays small. */
export interface InitialStateMessage {
  readonly kind: "initial_state";
  readonly patterns?: readonly ServerPatternEvent[];
  readonly session_labels?: Readonly<Record<string, SessionLabel>>;
  /** Per-session running shape counts at connect time. */
  readonly shape_counts?: Readonly<Record<string, ShapeCounts>>;
}

/** Running per-session shape tallies (mirrors Rust `ShapeCounts`). */
export interface ShapeCounts {
  readonly bash: number;
  readonly path: number;
  readonly change: number;
  readonly lines_added: number;
  readonly lines_removed: number;
  readonly programs: Readonly<Record<string, number>>;
  readonly top_segments: Readonly<Record<string, number>>;
}

/** Live shape projection for a session: new rows this batch + running counts. */
export interface ShapesMessage {
  readonly kind: "shapes";
  readonly session_id: string;
  readonly shapes: readonly ShapeRow[];
  readonly counts: ShapeCounts;
  readonly project_id?: string;
}

/** Mirrors Rust `ShapeRow`. */
export interface ShapeRow {
  readonly id: string;
  readonly session_id: string;
  readonly shape_type: string;
  readonly seq: number;
  readonly timestamp: string;
  readonly event_id: string;
  readonly data: Record<string, unknown>;
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
