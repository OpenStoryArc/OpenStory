/** SessionSummaryHeader — the shared at-a-glance header for a session.
 *
 *  One consistent strip of stats (model · turns · tools · duration · tokens ·
 *  errors · top file) rendered from the pure buildSessionSummary fold, reused
 *  across Explore / Overview so every session view reads as the same product.
 *  Errors are first-class: a distinct red stat you can't miss.
 */

import { useMemo } from "react";
import type { WireRecord } from "@/types/wire-record";
import { buildSessionSummary, type SessionSummary } from "@/lib/session-summary";
import { formatDuration, fullTimestamp } from "@/lib/time";
import { cn } from "@/lib/cn";

interface StripProps {
  summary: SessionSummary;
  className?: string;
  /** When set, the errors stat becomes a button that jumps to the first failure. */
  onJumpToError?: () => void;
  /** When set, the top-file stat becomes a button that filters events to it. */
  onFilterFile?: (path: string) => void;
}

interface Props extends Omit<StripProps, "summary"> {
  records: readonly WireRecord[];
}

function kfmt(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

function shortModel(model: string): string {
  return model.replace(/^claude-/, "").replace(/-\d{8}$/, "");
}

function Stat({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <span className="flex items-baseline gap-1">
      <span className="tabular-nums" style={color ? { color } : undefined}>{value}</span>
      {label && <span className="text-[#565f89]">{" "}{label}</span>}
    </span>
  );
}

/** Folds records into a summary, then renders the strip. Use SummaryStrip
 *  directly when the summary is already in hand (e.g. from /summary). */
export function SessionSummaryHeader({ records, ...rest }: Props) {
  const summary = useMemo(() => buildSessionSummary(records), [records]);
  return <SummaryStrip summary={summary} {...rest} />;
}

export function SummaryStrip({ summary: s, className, onJumpToError, onFilterFile }: StripProps) {
  const topFile = s.topFiles[0];

  return (
    <div className={cn("flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-[11px] text-[#c0caf5]", className)}>
      {s.model && (
        <span className="rounded bg-[#7aa2f7]/15 px-1.5 py-0.5 font-mono text-[10px] text-[#7aa2f7]">{shortModel(s.model)}</span>
      )}
      {s.durationMs > 0 && (
        <span
          data-testid="summary-duration"
          className="tabular-nums"
          title={
            s.startMs !== null && s.endMs !== null
              ? `${fullTimestamp(new Date(s.startMs).toISOString())} → ${fullTimestamp(new Date(s.endMs).toISOString())}`
              : undefined
          }
        >
          {formatDuration(s.durationMs)}
        </span>
      )}
      {s.turnCount > 0 && <Stat label={s.turnCount === 1 ? "turn" : "turns"} value={String(s.turnCount)} />}
      <Stat label={s.toolCount === 1 ? "tool" : "tools"} value={String(s.toolCount)} color="#7dcfff" />
      {s.totalTokens > 0 && <Stat label="tokens" value={kfmt(s.totalTokens)} color="#e0af68" />}
      {s.errorCount > 0 && (
        onJumpToError ? (
          <button
            type="button"
            data-testid="summary-errors"
            onClick={onJumpToError}
            className="flex items-baseline gap-1 text-[#f7768e] hover:underline"
            title="Jump to the first failure"
          >
            <span className="tabular-nums">{s.errorCount}</span>
            <span>{" "}failed →</span>
          </button>
        ) : s.firstErrorEventId && s.sessionId ? (
          // error→event: even without a scroll callback, "n failed" is a
          // PLACE — deep-link to the exact first-failure event.
          <a
            href={`#/explore/${s.sessionId}/event/${s.firstErrorEventId}`}
            data-testid="summary-errors"
            className="flex items-baseline gap-1 text-[#f7768e] hover:underline"
            title="Open the first failure"
          >
            <span className="tabular-nums">{s.errorCount}</span>
            <span>{" "}failed →</span>
          </a>
        ) : (
          <span data-testid="summary-errors" className="flex items-baseline gap-1 text-[#f7768e]">
            <span className="tabular-nums">{s.errorCount}</span>
            <span>{" "}failed</span>
          </span>
        )
      )}
      {s.parentSessionId && (
        <a
          href={`#/explore/${s.parentSessionId}`}
          data-testid="parent-session-link"
          className="text-[#bb9af7] hover:underline"
          title={`Spawned by session ${s.parentSessionId}`}
        >
          ↑ parent session
        </a>
      )}
      {topFile && (
        <span className="flex items-baseline gap-1 truncate text-[#565f89]">
          <span className="text-[#565f89]">·</span>
          {onFilterFile ? (
            <button
              type="button"
              data-testid="summary-top-file"
              onClick={() => onFilterFile(topFile.path)}
              className="truncate text-[#a9b1d6] hover:text-[#7dcfff] hover:underline"
              title={`Filter events to ${topFile.path}`}
            >
              {topFile.path.split("/").pop()}
            </button>
          ) : (
            <span className="truncate text-[#a9b1d6]" title={topFile.path}>{topFile.path.split("/").pop()}</span>
          )}
          {topFile.count > 1 && <span className="text-[#565f89]">×{topFile.count}</span>}
        </span>
      )}
    </div>
  );
}
