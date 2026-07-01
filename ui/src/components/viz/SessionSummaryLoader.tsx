/** Fetches a session's records and renders the shared SessionSummaryHeader.
 *  Lets surfaces that only hold a session id (e.g. Story) carry the same
 *  one-product spine as Explore and the Overview drill-in. */

import { useEffect, useState } from "react";
import type { WireRecord } from "@/types/wire-record";
import { SessionSummaryHeader } from "./SessionSummaryHeader";
import { Skeleton } from "@/components/ui/skeleton";

export function SessionSummaryLoader({ sessionId }: { sessionId: string }) {
  const [records, setRecords] = useState<WireRecord[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setRecords([]);
    fetch(`/api/sessions/${sessionId}/records`)
      .then((r) => r.json())
      .then((data: WireRecord[]) => {
        if (!cancelled) {
          setRecords(Array.isArray(data) ? data : []);
          setLoading(false);
        }
      })
      .catch(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
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
  if (records.length === 0) return null;
  return <SessionSummaryHeader records={records} />;
}
