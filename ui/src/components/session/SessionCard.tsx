import { memo, useCallback } from "react";
import type { SessionSummary } from "@/types/session";
import { relativeTimeFrom, formatDuration } from "@/lib/time";
import { truncate } from "@/lib/event-transforms";
import { STATUS_COLORS } from "@/lib/event-transforms";
import { tick$ } from "@/streams/clock";
import { useObservable } from "@/hooks/use-observable";
import { originAgentColor, originAgentLabel } from "@/lib/origin-agent";

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
  const agentLabel = originAgentLabel(session.origin_agent);
  const agentColor = originAgentColor(session.origin_agent);

  return (
    <button
      onClick={handleClick}
      className={`w-full text-left p-3 border-b border-[color:var(--bg-hover)] transition-colors ${
        selected ? "bg-[color:var(--bg-hover)]" : "hover:bg-[color:var(--bg-surface)]"
      }`}
    >
      <div className="flex items-center justify-between mb-1">
        <div className="flex items-center gap-1.5 min-w-0">
          {agentLabel && (
            <span
              className="text-xs font-medium px-1.5 py-0.5 rounded"
              style={{ color: agentColor, backgroundColor: `${agentColor}20` }}
              title={`Agent: ${agentLabel}`}
              data-testid="session-card-agent-badge"
            >
              {agentLabel}
            </span>
          )}
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
              <span className="text-[color:var(--text-muted)] font-normal ml-1">{elapsed}</span>
            )}
          </span>
        </div>
        <span className="text-xs text-[color:var(--text-muted)]">
          {timeAgo}
        </span>
      </div>
      <div className="text-xs text-[color:var(--text)] mb-1 leading-relaxed">
        {session.first_prompt
          ? truncate(session.first_prompt, 80)
          : "No prompt yet"}
      </div>
      <div className="flex items-center gap-2 text-xs text-[color:var(--text-muted)]">
        {session.model && <span>{session.model}</span>}
        <span>{session.event_count} events</span>
        {session.duration_ms != null && (
          <span>{formatDuration(session.duration_ms)}</span>
        )}
      </div>
    </button>
  );
});
