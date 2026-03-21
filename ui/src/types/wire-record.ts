import type { RecordType, RecordPayload } from "./view-record";

/** WireRecord = ViewRecord + tree metadata + truncation info.
 *  This is the wire format sent over WebSocket from the server.
 *  Mirrors the Rust WireRecord struct. */
export interface WireRecord {
  readonly id: string;
  readonly seq: number;
  readonly session_id: string;
  readonly timestamp: string;
  readonly record_type: RecordType;
  readonly payload: RecordPayload;
  /** Subagent identity (null = main agent). From ViewRecord via serde(flatten). */
  readonly agent_id: string | null;
  /** Whether this event belongs to a sidechain (subagent file). */
  readonly is_sidechain: boolean;
  readonly depth: number;
  readonly parent_uuid: string | null;
  readonly truncated: boolean;
  readonly payload_bytes: number;
}

/** Pattern detected by streaming pattern detectors. */
export interface PatternView {
  readonly type: string;
  readonly label: string;
  readonly events: readonly string[];
  readonly metadata?: Readonly<Record<string, unknown>>;
}
