import { useState, useCallback, useMemo, memo } from "react";
import type { SessionSummary } from "@/types/session";
import { SessionCard } from "./SessionCard";
import { projectDisplayName } from "@/lib/filters";
import { relativeTimeFrom } from "@/lib/time";
import { tick$ } from "@/streams/clock";
import { useObservable } from "@/hooks/use-observable";

interface ProjectGroupProps {
  projectId: string;
  sessions: readonly SessionSummary[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

/** Check if a group has had activity within the last 24 hours */
function hasRecentActivity(sessions: readonly SessionSummary[]): boolean {
  if (sessions.length === 0) return false;
  const latest = sessions[0]?.start_time; // sessions are pre-sorted, newest first
  if (!latest) return false;
  const age = Date.now() - new Date(latest).getTime();
  return age < 24 * 60 * 60 * 1000;
}

export const ProjectGroup = memo(function ProjectGroup({
  projectId,
  sessions,
  selectedId,
  onSelect,
}: ProjectGroupProps) {
  const hasSelected = sessions.some((s) => s.session_id === selectedId);
  const recent = hasRecentActivity(sessions);
  const [expanded, setExpanded] = useState(recent || hasSelected);
  const toggle = useCallback(() => setExpanded((e) => !e), []);
  const displayName = projectDisplayName(projectId, sessions);
  const now = useObservable(tick$, Date.now());

  const { latestTime, ongoingCount } = useMemo(() => {
    const latest = sessions[0]?.start_time;
    const ongoing = sessions.filter((s) => s.status === "ongoing").length;
    return { latestTime: latest, ongoingCount: ongoing };
  }, [sessions]);

  return (
    <div>
      <button
        onClick={toggle}
        className="w-full flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-[#7aa2f7] hover:bg-[#24283b] transition-colors"
      >
        <span className="text-[#565f89]">{expanded ? "\u25BE" : "\u25B8"}</span>
        <span className="truncate flex-1 text-left">
          {displayName}
          {ongoingCount > 0 && (
            <span className="ml-1.5 text-[#9ece6a]">{"\u25CF"}</span>
          )}
        </span>
        <span className="text-[#565f89] tabular-nums text-right">
          <span>{sessions.length}</span>
          {latestTime && (
            <span className="ml-1.5 opacity-60">{relativeTimeFrom(latestTime, now)}</span>
          )}
        </span>
      </button>
      {expanded &&
        sessions.map((s) => (
          <SessionCard
            key={s.session_id}
            session={s}
            selected={s.session_id === selectedId}
            onSelect={onSelect}
          />
        ))}
    </div>
  );
});
