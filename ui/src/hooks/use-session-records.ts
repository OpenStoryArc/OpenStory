/** useSessionRecords — every surface reads a session's records from ONE
 *  shared fetch.
 *
 *  Story's summary header, Explore's viz and timeline all need the same
 *  whole-session record array; before this hook each fetched
 *  /api/sessions/{id}/records independently (91 MB × 2–4 on a big session).
 *  The module-level cache means the first mounted consumer pays the fetch
 *  and the rest subscribe to the same promise.
 *
 *  Bounded: the fetch walks SQL-paginated pages (90 ms each since 6755959)
 *  newest-first and stops at RECORD_CAP — a 100k-event session must not
 *  swallow the browser's heap (measured: unbounded fetch+render = 983 MB /
 *  34 s). Views read `capped` and say "showing the latest N" honestly.
 */

import { useEffect, useState } from "react";
import type { WireRecord } from "@/types/wire-record";
import { createKeyedPromiseCache, liveInvalidationKey } from "@/lib/record-cache";
import { fetchRecentSessionRecords } from "@/lib/session-records";
import { wsMessages$ } from "@/streams/connection";

/** Most-recent records a whole-session read model holds. ~25k keeps even a
 *  100k-event session's parsed JSON in the low hundreds of MB. */
export const RECORD_CAP = 25_000;

export const sessionRecordsCache = createKeyedPromiseCache<{
  records: WireRecord[];
  capped: boolean;
}>((id) => fetchRecentSessionRecords(id, RECORD_CAP, { pageSize: 2000 }));

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
  /** True when older history exists beyond RECORD_CAP — say so in the UI. */
  readonly capped: boolean;
}

export function useSessionRecords(sessionId: string | null): SessionRecordsState {
  const [state, setState] = useState<SessionRecordsState>({
    records: [],
    loading: sessionId !== null,
    error: null,
    capped: false,
  });

  useEffect(() => {
    if (!sessionId) {
      setState({ records: [], loading: false, error: null, capped: false });
      return;
    }
    armLiveInvalidation();
    let cancelled = false;
    setState({ records: [], loading: true, error: null, capped: false });
    sessionRecordsCache.get(sessionId).then(
      ({ records, capped }) => {
        if (!cancelled) setState({ records, loading: false, error: null, capped });
      },
      (e: Error) => {
        if (!cancelled) setState({ records: [], loading: false, error: e, capped: false });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  return state;
}
