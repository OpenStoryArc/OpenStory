/** Renders the shared summary strip from GET /api/sessions/{id}/summary —
 *  a sub-millisecond ~600 B projection read — instead of fetching the
 *  whole-session records (99 MB on a big session) just to fold them down
 *  to seven stats. Lets surfaces that only hold a session id (e.g. Story)
 *  carry the same one-product spine as Explore and the Overview drill-in. */

import { useEffect, useState } from "react";
import { SummaryStrip } from "./SessionSummaryHeader";
import { Skeleton } from "@/components/ui/skeleton";
import {
  summaryFromApi,
  type ApiSessionSummary,
  type SessionSummary,
} from "@/lib/session-summary";
import { createKeyedPromiseCache, liveInvalidationKey } from "@/lib/record-cache";
import { wsMessages$ } from "@/streams/connection";

export const sessionSummaryCache = createKeyedPromiseCache<SessionSummary>(
  async (id) => {
    const res = await fetch(`/api/sessions/${encodeURIComponent(id)}/summary`);
    if (!res.ok) throw new Error(`summary ${id}: ${res.status}`);
    return summaryFromApi((await res.json()) as ApiSessionSummary);
  },
);

// A live session's summary changes with every broadcast — drop the cached
// entry so the next mount refetches. Armed lazily, once.
let invalidationArmed = false;
function armLiveInvalidation() {
  if (invalidationArmed) return;
  invalidationArmed = true;
  wsMessages$().subscribe((msg) => {
    const key = liveInvalidationKey(msg);
    if (key) sessionSummaryCache.invalidate(key);
  });
}

export function SessionSummaryLoader({ sessionId }: { sessionId: string }) {
  const [summary, setSummary] = useState<SessionSummary | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    armLiveInvalidation();
    let cancelled = false;
    setLoading(true);
    setSummary(null);
    sessionSummaryCache.get(sessionId).then(
      (s) => {
        if (!cancelled) {
          setSummary(s);
          setLoading(false);
        }
      },
      () => {
        if (!cancelled) setLoading(false);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  if (loading) {
    return (
      <div className="flex gap-3 px-3 py-2" data-testid="summary-loading">
        <Skeleton className="h-4 w-16" />
        <Skeleton className="h-4 w-12" />
        <Skeleton className="h-4 w-14" />
        <Skeleton className="h-4 w-12" />
      </div>
    );
  }
  if (!summary || summary.eventCount === 0) return null;
  return <SummaryStrip summary={summary} />;
}
