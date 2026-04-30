/**
 * Per-session record fetching — REST GET /api/sessions/{id}/records.
 *
 * Used by the lazy-load path: when the user opens a session in the
 * Timeline, the component calls `fetchSessionRecords(sid)` and dispatches
 * the result through `dispatchSessionRecordsLoaded`. Live updates for
 * the same session continue to flow over WebSocket and merge into the
 * same flat records array (deduped by id in the reducer).
 *
 * Pagination cursor: the server returns oldest-first within the
 * requested window. To fetch the page above, pass the seq of the first
 * (oldest) returned record as `beforeSeq`.
 */

import type { WireRecord } from "@/types/wire-record";

export interface FetchOptions {
  /** Max records to return. Server clamps to [1, 2000]. Default 500. */
  readonly limit?: number;
  /** Filter to records with seq < beforeSeq. Used to walk history backward. */
  readonly beforeSeq?: number;
  /** AbortSignal for cancelling in-flight fetches on session change. */
  readonly signal?: AbortSignal;
}

/** Fetch a page of WireRecords for `sessionId`. Throws on network or
 *  parse failure; returns `[]` when the session has no records. */
export async function fetchSessionRecords(
  sessionId: string,
  opts: FetchOptions = {},
): Promise<WireRecord[]> {
  const params = new URLSearchParams();
  if (opts.limit != null) params.set("limit", String(opts.limit));
  if (opts.beforeSeq != null) params.set("before_seq", String(opts.beforeSeq));

  // Pagination is opt-in: omit the query string entirely when no params
  // are provided so the response matches the legacy "all records" shape
  // existing callers (SessionTimeline, TurnCard) already consume.
  const qs = params.toString();
  const url = qs
    ? `/api/sessions/${encodeURIComponent(sessionId)}/records?${qs}`
    : `/api/sessions/${encodeURIComponent(sessionId)}/records`;

  const res = await fetch(url, { signal: opts.signal });
  if (!res.ok) {
    throw new Error(`fetchSessionRecords ${sessionId}: ${res.status} ${res.statusText}`);
  }
  const data: unknown = await res.json();
  if (!Array.isArray(data)) {
    throw new Error(`fetchSessionRecords ${sessionId}: response was not an array`);
  }
  return data as WireRecord[];
}

/** Default page size used by the Timeline lazy-load path. Matches the
 *  server-side default; surfaced here so the UI can compute the next
 *  cursor without hard-coding a magic number. */
export const DEFAULT_PAGE_SIZE = 500;
