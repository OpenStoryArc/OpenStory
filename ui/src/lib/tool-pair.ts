/** The toolcall↔result pairing — one round trip joined by call_id.
 *
 *  Pure fold: record id → the EVENT id of its partner (both directions),
 *  so a tool_call card can jump to its result and back. Unanswered calls
 *  and orphan results stay unmapped — no phantom partners.
 */

import type { WireRecord } from "@/types/wire-record";

interface CallIdPayload {
  readonly call_id?: string;
}

export function toolPairMap(records: readonly WireRecord[]): Map<string, string> {
  const calls = new Map<string, string>();
  const results = new Map<string, string>();
  for (const r of records) {
    const callId = (r.payload as CallIdPayload)?.call_id;
    if (!callId) continue;
    if (r.record_type === "tool_call") calls.set(callId, r.id);
    else if (r.record_type === "tool_result") results.set(callId, r.id);
  }
  const pairs = new Map<string, string>();
  for (const [callId, callEventId] of calls) {
    const resultEventId = results.get(callId);
    if (resultEventId) {
      pairs.set(callEventId, resultEventId);
      pairs.set(resultEventId, callEventId);
    }
  }
  return pairs;
}
