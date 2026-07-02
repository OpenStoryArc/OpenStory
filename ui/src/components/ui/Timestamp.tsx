/** Timestamp — the canonical "WHEN" chip. Shows a relative time ("43d ago")
 *  and reveals the absolute local datetime on hover (title). Use this for every
 *  session-related card so time is consistent and always fully reachable — no
 *  time-only-without-a-date, no dead-end. Pure over lib/time; re-renders pick up
 *  the passing of time on the next paint. */

import { relativeTime, absoluteTime } from "@/lib/time";

export function Timestamp({
  iso,
  className,
  prefix,
}: {
  iso?: string | null;
  className?: string;
  /** optional leading glyph/label, e.g. "🕘" or "updated". */
  prefix?: string;
}) {
  if (!iso) return null;
  return (
    <span className={className} title={absoluteTime(iso)} data-testid="timestamp">
      {prefix ? `${prefix} ` : ""}
      {relativeTime(iso)}
    </span>
  );
}
