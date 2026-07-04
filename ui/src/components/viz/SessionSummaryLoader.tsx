/** Renders the shared SessionSummaryHeader from the shared record cache.
 *  Lets surfaces that only hold a session id (e.g. Story) carry the same
 *  one-product spine as Explore and the Overview drill-in — without paying
 *  a second whole-session fetch when another surface already loaded it. */

import { SessionSummaryHeader } from "./SessionSummaryHeader";
import { Skeleton } from "@/components/ui/skeleton";
import { useSessionRecords } from "@/hooks/use-session-records";

export function SessionSummaryLoader({ sessionId }: { sessionId: string }) {
  const { records, loading } = useSessionRecords(sessionId);

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
