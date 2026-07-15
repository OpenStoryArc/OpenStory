/** SubagentsSection — the subagents a session spawned, finally shown in its
 *  report. Each row links to the child `agent-*` session so you can drill from
 *  a parent into the work it delegated (previously orphaned in the flat list). */

import { useMemo } from "react";
import type { WireRecord } from "@/types/wire-record";
import { extractSubagents } from "@/lib/subagents";
import { sessionColor } from "@/lib/session-colors";
import { cn } from "@/lib/cn";

interface Props {
  records: readonly WireRecord[];
  onOpen?: (sessionId: string) => void;
  className?: string;
}

export function SubagentsSection({ records, onOpen, className }: Props) {
  const subs = useMemo(() => extractSubagents(records), [records]);
  if (subs.length === 0) return null;

  return (
    <div className={cn("px-3 py-2", className)} data-testid="subagents-section">
      <div className="mb-1 text-[length:var(--fs-label)] font-medium uppercase tracking-wide text-[color:var(--text-muted)]">
        Subagents · {subs.length}
      </div>
      <div className="flex flex-col gap-0.5">
        {subs.map((s) => {
          const linkable = Boolean(s.sessionId && onOpen);
          const color = s.sessionId ? sessionColor(s.sessionId) : "#565f89";
          return (
            <button
              key={s.callId}
              data-subagent={s.sessionId ?? s.callId}
              disabled={!linkable}
              onClick={linkable ? () => onOpen!(s.sessionId!) : undefined}
              className={cn(
                "flex items-center gap-2 rounded px-2 py-1 text-left text-[length:var(--fs-body)] transition-colors",
                linkable ? "hover:bg-[color:var(--bg-surface)] cursor-pointer" : "cursor-default opacity-80",
              )}
              title={linkable ? "Open this subagent's session" : "Subagent session not linked yet"}
            >
              <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: s.isError ? "#f7768e" : color }} />
              {s.subagentType && (
                <span className="shrink-0 rounded bg-[color:var(--purple)]/15 px-1 text-[length:var(--fs-label)] text-[color:var(--purple)]">{s.subagentType}</span>
              )}
              <span className="min-w-0 flex-1 truncate text-[color:var(--text)]">{s.description}</span>
              {s.isError && <span className="shrink-0 text-[length:var(--fs-label)] text-[color:var(--red)]">failed</span>}
              {linkable && <span className="shrink-0 text-[color:var(--text-muted)]">→</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}
