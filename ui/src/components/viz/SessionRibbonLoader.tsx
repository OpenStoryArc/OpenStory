/** Fetches a session's records and renders its activity ribbon.
 *  Used where only a session id is in hand (e.g. the Overview drill-in). */

import { useEffect, useState } from "react";
import type { WireRecord } from "@/types/wire-record";
import { SessionActivityRibbon } from "./SessionActivityRibbon";

export function SessionRibbonLoader({ sessionId }: { sessionId: string }) {
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

  if (loading) return <div className="px-3 py-3 text-[11px] text-[#565f89]">Loading activity…</div>;
  return <SessionActivityRibbon records={records} />;
}
