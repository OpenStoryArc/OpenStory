/** Timestamped error messages for a session. */

import { compactTime, fullTimestamp } from "@/lib/time";
import { truncateError, type SessionError } from "@/lib/session-detail";

interface ErrorListProps {
  errors: readonly SessionError[];
}

export function ErrorList({ errors }: ErrorListProps) {
  if (errors.length === 0) return null;

  return (
    <div data-testid="error-list">
      <div className="text-[10px] text-[color:var(--red)] mb-1.5">
        Errors ({errors.length})
      </div>
      <div className="space-y-1 max-h-[150px] overflow-y-auto">
        {errors.map((e, i) => (
          <div
            key={`${e.timestamp}-${i}`}
            className="text-xs bg-[color:var(--red)]/6 rounded px-2 py-1.5 font-mono"
          >
            <span className="text-[color:var(--text-muted)] mr-2" title={fullTimestamp(e.timestamp)}>{compactTime(e.timestamp)}</span>
            <span className="text-[color:var(--red)]">{truncateError(e.message)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
