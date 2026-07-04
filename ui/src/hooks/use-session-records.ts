/** useSessionRecords — every surface reads a session's records from ONE
 *  shared fetch.
 *
 *  Story's summary header, Explore's viz and timeline all need the same
 *  whole-session record array; before this hook each fetched
 *  /api/sessions/{id}/records independently (91 MB × 2–4 on a big session).
 *  The module-level cache means the first mounted consumer pays the fetch
 *  and the rest subscribe to the same promise.
 *
 *  Deliberately ONE unpaginated fetch, not the page walker: measured on a
 *  16.7k-event session (2026-07-04), every /records page pays a ~0.9 s
 *  fixed cost (the server scans the whole session then slices), so a
 *  500/page walk would take ~30 s vs 2.0 s for the single fetch. Until
 *  pagination is pushed into SQL, sharing the one big fetch is the win.
 */

import { useEffect, useState } from "react";
import type { WireRecord } from "@/types/wire-record";
import { createKeyedPromiseCache, liveInvalidationKey } from "@/lib/record-cache";
import { fetchSessionRecords } from "@/lib/session-records";
import { wsMessages$ } from "@/streams/connection";

export const sessionRecordsCache = createKeyedPromiseCache<WireRecord[]>((id) =>
  fetchSessionRecords(id),
);

// Live sessions grow: any view_records broadcast for a session drops its
// cached snapshot, so the next consumer refetches fresh. Armed once, lazily,
// so importing this module in tests carries no subscription side effect.
let invalidationArmed = false;
function armLiveInvalidation() {
  if (invalidationArmed) return;
  invalidationArmed = true;
  wsMessages$().subscribe((msg) => {
    const key = liveInvalidationKey(msg);
    if (key) sessionRecordsCache.invalidate(key);
  });
}

export interface SessionRecordsState {
  readonly records: WireRecord[];
  readonly loading: boolean;
  readonly error: Error | null;
}

export function useSessionRecords(sessionId: string | null): SessionRecordsState {
  const [state, setState] = useState<SessionRecordsState>({
    records: [],
    loading: sessionId !== null,
    error: null,
  });

  useEffect(() => {
    if (!sessionId) {
      setState({ records: [], loading: false, error: null });
      return;
    }
    armLiveInvalidation();
    let cancelled = false;
    setState({ records: [], loading: true, error: null });
    sessionRecordsCache.get(sessionId).then(
      (records) => {
        if (!cancelled) setState({ records, loading: false, error: null });
      },
      (e: Error) => {
        if (!cancelled) setState({ records: [], loading: false, error: e });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  return state;
}
