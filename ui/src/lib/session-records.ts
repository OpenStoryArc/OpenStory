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

/** Safety cap on cursor-walk iterations. Each page is at most
 *  `MAX_RECORDS_LIMIT = 2000` server-side, so 200 iterations covers
 *  400k-record sessions before bailing — well above any realistic
 *  Claude Code transcript. */
const MAX_PAGES = 200;

/** Stream every record for `sessionId` page by page, newest-window-first.
 *
 *  Each yielded page is at most `pageSize` records, oldest-first within
 *  the window. The first page covers the most-recent records; subsequent
 *  pages walk older history via the `before_seq` cursor until a short
 *  (or empty) page comes back.
 *
 *  Why newest-first: the user just opened the session, the recent activity
 *  is what they care about most. Yielding it first lets the UI paint after
 *  one round-trip while older pages keep streaming in the background. */
export async function* streamSessionRecords(
  sessionId: string,
  opts: { pageSize?: number; signal?: AbortSignal } = {},
): AsyncGenerator<WireRecord[], void, void> {
  const pageSize = opts.pageSize ?? DEFAULT_PAGE_SIZE;
  let beforeSeq: number | undefined;

  for (let i = 0; i < MAX_PAGES; i++) {
    if (opts.signal?.aborted) {
      throw new DOMException("Aborted", "AbortError");
    }
    const page = await fetchSessionRecords(sessionId, {
      limit: pageSize,
      beforeSeq,
      signal: opts.signal,
    });
    if (page.length === 0) return;
    yield page;
    if (page.length < pageSize) return;
    beforeSeq = page[0]!.seq;
    if (beforeSeq <= 1) return;
  }
}

/** Fetch every record for `sessionId`, walking the `before_seq` cursor
 *  backward until a short page comes back. Returns one flat array
 *  oldest-first.
 *
 *  Prefer `streamSessionRecords` when the consumer can handle progressive
 *  dispatch — it lets the UI paint after the first round-trip rather than
 *  waiting for the full walk. This helper exists for callers that need
 *  the materialized array (e.g. tests, batch analysis). */
export async function fetchAllSessionRecords(
  sessionId: string,
  opts: { pageSize?: number; signal?: AbortSignal } = {},
): Promise<WireRecord[]> {
  const pages: WireRecord[][] = [];
  for await (const page of streamSessionRecords(sessionId, opts)) {
    pages.push(page);
  }
  // Pages came newest-window-first; flatten oldest-first.
  const out: WireRecord[] = [];
  for (let i = pages.length - 1; i >= 0; i--) out.push(...pages[i]!);
  return out;
}
