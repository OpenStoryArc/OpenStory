/** Timestamped error messages for a session. */

import { compactTime } from "@/lib/time";
import { truncateError, type SessionError } from "@/lib/session-detail";

interface ErrorListProps {
  errors: readonly SessionError[];
}

export function ErrorList({ errors }: ErrorListProps) {
  if (errors.length === 0) return null;

  return (
    <div data-testid="error-list">
      <div className="text-[10px] text-[#f7768e] mb-1.5">
        Errors ({errors.length})
      </div>
      <div className="space-y-1 max-h-[150px] overflow-y-auto">
        {errors.map((e, i) => (
          <div
            key={`${e.timestamp}-${i}`}
            className="text-xs bg-[#f7768e10] rounded px-2 py-1.5 font-mono"
          >
            <span className="text-[#565f89] mr-2">{compactTime(e.timestamp)}</span>
            <span className="text-[#f7768e]">{truncateError(e.message)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
