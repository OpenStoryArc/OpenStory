/** TokenReport — the full token breakdown for a session.
 *
 *  The headline "tokens" number elsewhere long excluded the prompt cache, which
 *  for Claude sessions dominates (cache reads run 100-200× the fresh input). This
 *  report shows every category — output, input, cache write, cache read — as a
 *  proportional stacked bar plus exact counts and a cache-hit-rate, so the true
 *  scale (and how much context was reused vs. re-sent) is finally visible.
 */

import { useMemo } from "react";
import type { WireRecord } from "@/types/wire-record";
import { buildSessionSummary } from "@/lib/session-summary";
import { cn } from "@/lib/cn";

interface CacheInput {
  inputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
}

/** Share of input tokens served from cache vs. sent fresh (0..1). */
export function cacheHitRate(s: CacheInput): number {
  const totalInput = s.inputTokens + s.cacheCreationTokens + s.cacheReadTokens;
  return totalInput > 0 ? s.cacheReadTokens / totalInput : 0;
}

function compact(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  if (n < 1_000_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  return `${(n / 1_000_000_000).toFixed(2)}B`;
}

const CATS = [
  { key: "output", label: "Output", color: "#9ece6a", hint: "tokens the agent generated" },
  { key: "input", label: "Input", color: "#7aa2f7", hint: "fresh (uncached) input" },
  { key: "cacheWrite", label: "Cache write", color: "#e0af68", hint: "context written to cache" },
  { key: "cacheRead", label: "Cache read", color: "#414868", hint: "context served from cache (cheap, dominant)" },
] as const;

export function TokenReport({ records, className }: { records: readonly WireRecord[]; className?: string }) {
  const s = useMemo(() => buildSessionSummary(records), [records]);

  const values: Record<string, number> = {
    output: s.outputTokens,
    input: s.inputTokens,
    cacheWrite: s.cacheCreationTokens,
    cacheRead: s.cacheReadTokens,
  };
  const total = s.totalTokens;

  if (total === 0) {
    return <div className={cn("px-3 py-4 text-[11px] text-[color:var(--text-muted)]", className)}>No token data for this session.</div>;
  }

  const hit = cacheHitRate(s);

  return (
    <div className={cn("px-3 py-2", className)}>
      <div className="mb-2 flex items-baseline justify-between">
        <div>
          <span className="text-[18px] font-semibold tabular-nums text-[color:var(--text)]">{compact(total)}</span>
          <span className="ml-1 text-[10px] text-[color:var(--text-muted)]">tokens total</span>
        </div>
        <div className="text-right">
          <span className="text-[13px] font-medium tabular-nums text-[color:var(--cyan-bright)]">{Math.round(hit * 100)}%</span>
          <span className="ml-1 text-[10px] text-[color:var(--text-muted)]">from cache</span>
        </div>
      </div>

      {/* proportional stacked bar (min-width so tiny categories stay visible) */}
      <div className="flex h-3 w-full overflow-hidden rounded" role="img" aria-label="Token breakdown">
        {CATS.map((c) => {
          const v = values[c.key] ?? 0;
          if (v <= 0) return null;
          const pct = (v / total) * 100;
          return (
            <div
              key={c.key}
              data-token-seg={c.key}
              className="h-full"
              style={{ width: `${pct}%`, minWidth: 3, background: c.color }}
              title={`${c.label}: ${v.toLocaleString()} (${pct.toFixed(pct < 1 ? 2 : 1)}%)`}
            />
          );
        })}
      </div>

      {/* legend / exact counts */}
      <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1">
        {CATS.map((c) => {
          const v = values[c.key] ?? 0;
          const pct = total > 0 ? (v / total) * 100 : 0;
          return (
            <div key={c.key} className="flex items-center gap-1.5 text-[11px]" title={c.hint}>
              <span className="h-2 w-2 shrink-0 rounded-sm" style={{ background: c.color }} />
              <span className="text-[color:var(--text-bright)]">{c.label}</span>
              <span className="ml-auto tabular-nums text-[color:var(--text)]">{v.toLocaleString()}</span>
              <span className="w-9 text-right tabular-nums text-[color:var(--text-muted)]">{pct.toFixed(pct < 1 && pct > 0 ? 2 : 0)}%</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
