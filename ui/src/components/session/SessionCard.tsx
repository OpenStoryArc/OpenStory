import { memo, useCallback } from "react";
import type { SessionSummary } from "@/types/session";
import { relativeTimeFrom, formatDuration } from "@/lib/time";
import { truncate } from "@/lib/event-transforms";
import { STATUS_COLORS } from "@/lib/event-transforms";
import { tick$ } from "@/streams/clock";
import { useObservable } from "@/hooks/use-observable";

interface SessionCardProps {
  session: SessionSummary;
  selected: boolean;
  onSelect: (id: string) => void;
}

export const SessionCard = memo(function SessionCard({
  session,
  selected,
  onSelect,
}: SessionCardProps) {
  const now = useObservable(tick$, Date.now());

  const handleClick = useCallback(() => {
    onSelect(session.session_id);
  }, [session.session_id, onSelect]);

  const statusColor = STATUS_COLORS[session.status] ?? "#565f89";
  const timeAgo = relativeTimeFrom(session.start_time, now);
  const isActive = session.status === "ongoing";
  const elapsed = isActive
    ? formatDuration(now - new Date(session.start_time).getTime())
    : null;
  const isStale = session.status === "stale";

  return (
    <button
      onClick={handleClick}
      className={`w-full text-left p-3 border-b border-[#2f3348] transition-colors ${
        selected ? "bg-[#2f3348]" : "hover:bg-[#24283b]"
      }`}
    >
      <div className="flex items-center justify-between mb-1">
        <span
          className={`text-xs font-medium px-1.5 py-0.5 rounded inline-flex items-center gap-1${isStale ? " opacity-60" : ""}`}
          style={{ color: statusColor, backgroundColor: `${statusColor}20` }}
        >
          {isActive && (
            <span
              className="inline-block w-1.5 h-1.5 rounded-full pulse-live"
              style={{ backgroundColor: statusColor }}
            />
          )}
          {session.status}
          {elapsed && (
            <span className="text-[#565f89] font-normal ml-1">{elapsed}</span>
          )}
        </span>
        <span className="text-xs text-[#565f89]">
          {timeAgo}
        </span>
      </div>
      <div className="text-xs text-[#c0caf5] mb-1 leading-relaxed">
        {session.first_prompt
          ? truncate(session.first_prompt, 80)
          : "No prompt yet"}
      </div>
      <div className="flex items-center gap-2 text-xs text-[#565f89]">
        {session.model && <span>{session.model}</span>}
        <span>{session.event_count} events</span>
        {session.duration_ms != null && (
          <span>{formatDuration(session.duration_ms)}</span>
        )}
      </div>
    </button>
  );
});
