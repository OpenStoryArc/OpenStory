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
      {label && <span className="text-[color:var(--text-muted)]">{" "}{label}</span>}
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
    <div className={cn("flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-[11px] text-[color:var(--text)]", className)}>
      {s.model && (
        <span className="rounded bg-[color:var(--accent)]/15 px-1.5 py-0.5 font-mono text-[10px] text-[color:var(--accent)]">{shortModel(s.model)}</span>
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
      {/* Stat values stay neutral (Stripe discipline): color in this strip is
          reserved for meaning, not per-metric identity — the rainbow read as
          noise. The model chip keeps the single interactive accent. */}
      {s.turnCount > 0 && <Stat label={s.turnCount === 1 ? "turn" : "turns"} value={String(s.turnCount)} />}
      <Stat label={s.toolCount === 1 ? "tool" : "tools"} value={String(s.toolCount)} />
      {s.totalTokens > 0 && <Stat label="tokens" value={kfmt(s.totalTokens)} />}
      {s.errorCount > 0 && (
        onJumpToError ? (
          <button
            type="button"
            data-testid="summary-errors"
            onClick={onJumpToError}
            className="flex items-baseline gap-1 text-[color:var(--text-muted)] hover:underline"
            title="Jump to the first failure"
          >
            <span className="tabular-nums">{s.errorCount}</span>
            <span>{" "}failed tools →</span>
          </button>
        ) : s.firstErrorEventId && s.sessionId ? (
          // error→event: even without a scroll callback, "n failed" is a
          // PLACE — deep-link to the exact first-failure event.
          <a
            href={`#/explore/${s.sessionId}/event/${s.firstErrorEventId}`}
            data-testid="summary-errors"
            className="flex items-baseline gap-1 text-[color:var(--text-muted)] hover:underline"
            title="Open the first failure"
          >
            <span className="tabular-nums">{s.errorCount}</span>
            <span>{" "}failed tools →</span>
          </a>
        ) : (
          <span data-testid="summary-errors" className="flex items-baseline gap-1 text-[color:var(--text-muted)]">
            <span className="tabular-nums">{s.errorCount}</span>
            <span>{" "}failed tools</span>
          </span>
        )
      )}
      {s.parentSessionId && (
        <a
          href={`#/explore/${s.parentSessionId}`}
          data-testid="parent-session-link"
          className="text-[color:var(--accent)] hover:underline"
          title={`Spawned by session ${s.parentSessionId}`}
        >
          ↑ parent session
        </a>
      )}
      {topFile && (
        <span className="flex items-baseline gap-1 truncate text-[color:var(--text-muted)]">
          <span className="text-[color:var(--text-muted)]">·</span>
          {onFilterFile ? (
            <button
              type="button"
              data-testid="summary-top-file"
              onClick={() => onFilterFile(topFile.path)}
              className="truncate text-[color:var(--text-bright)] hover:text-[color:var(--cyan-bright)] hover:underline"
              title={`Filter events to ${topFile.path}`}
            >
              {topFile.path.split("/").pop()}
            </button>
          ) : (
            <span className="truncate text-[color:var(--text-bright)]" title={topFile.path}>{topFile.path.split("/").pop()}</span>
          )}
          {topFile.count > 1 && <span className="text-[color:var(--text-muted)]">×{topFile.count}</span>}
        </span>
      )}
    </div>
  );
}
