/** Layout-matched loading skeletons for the sessions views (Overview-era, now shared).
 *  Each mirrors the shape of the real content it replaces so nothing shifts
 *  when data arrives. */

import { Skeleton } from "@/components/ui/skeleton";

function SessionRowSkeleton() {
  return (
    <div className="flex items-center gap-3 border-b border-[color:var(--bg-hover)]/60 px-3 py-2">
      <Skeleton className="h-2.5 w-2.5 rounded-full" />
      <div className="min-w-0 flex-1 space-y-1.5">
        <Skeleton className="h-3 w-1/2" />
        <Skeleton className="h-2 w-1/3" />
      </div>
      <div className="flex flex-col items-end gap-1.5">
        <Skeleton className="h-2 w-16" />
        <Skeleton className="h-2 w-12" />
      </div>
    </div>
  );
}

export function SessionListSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <div data-testid="session-list-skeleton" aria-busy="true" aria-label="Loading sessions">
      {Array.from({ length: rows }, (_, i) => (
        <SessionRowSkeleton key={i} />
      ))}
    </div>
  );
}

/** Skeleton for the drill-in viz block (summary strip + ribbon + trace rows). */
export function SessionVizSkeleton() {
  return (
    <div data-testid="session-viz-skeleton" aria-busy="true" className="space-y-3 p-3">
      <div className="flex gap-3">
        <Skeleton className="h-4 w-16" />
        <Skeleton className="h-4 w-12" />
        <Skeleton className="h-4 w-14" />
        <Skeleton className="h-4 w-12" />
      </div>
      <Skeleton className="h-16 w-full" />
      <div className="space-y-1.5">
        {Array.from({ length: 5 }, (_, i) => (
          <Skeleton key={i} className="h-3" style={{ width: `${90 - i * 12}%` }} />
        ))}
      </div>
    </div>
  );
}
