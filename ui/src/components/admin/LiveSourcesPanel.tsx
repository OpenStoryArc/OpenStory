/**
 * LiveSourcesPanel — authoritative fleet roster from JetStream.
 *
 * Visible only when the topology endpoint includes `live_sources`
 * (federation-mode hub with `Bus::jetstream()` reachable). Shows each
 * registered leaf with its identity (`host` parsed from
 * `external.api_prefix`), runtime delivery state (`lag`, `active_ms`),
 * and a stale indicator when `active_ms` exceeds a few seconds.
 *
 * Empty array: JetStream is up but no leaves have registered yet
 * (just-booted hub). Null/undefined: not in a hub-visible mode.
 */

import type { LiveSourceSummary } from "@/lib/admin-api";

interface Props {
  sources: readonly LiveSourceSummary[];
}

export function LiveSourcesPanel({ sources }: Props) {
  if (sources.length === 0) {
    return (
      <p className="text-sm text-[color:var(--text-muted)]">
        JetStream is reachable, but <code>events-agg</code> has no
        registered sources yet. Leaves will appear here as they
        self-register via the hub's cross-domain API.
      </p>
    );
  }
  return (
    <table className="w-full text-sm" data-testid="live-sources-table">
      <thead>
        <tr className="text-left text-xs uppercase tracking-wider text-[color:var(--text-muted)] border-b border-[color:var(--bg-surface)]">
          <th className="py-2 pr-4">Source</th>
          <th className="py-2 pr-4">Host</th>
          <th className="py-2 pr-4">Lag</th>
          <th className="py-2 pr-4">Active</th>
        </tr>
      </thead>
      <tbody>
        {sources.map((s, i) => {
          const stale =
            s.active_ms !== null && s.active_ms > 30_000;
          return (
            <tr key={`${s.name}-${i}`} className="border-b border-[#16161e]">
              <td className="py-2 pr-4">
                <code className="text-[color:var(--accent)]">{s.name}</code>
              </td>
              <td className="py-2 pr-4">
                {s.host ? (
                  <code className="text-[color:var(--purple)]">{s.host}</code>
                ) : (
                  <span className="text-[color:var(--text-muted)]" title={s.api_prefix ?? ""}>
                    (local / unparseable)
                  </span>
                )}
              </td>
              <td className="py-2 pr-4">
                <span
                  className={
                    s.lag === 0 ? "text-[color:var(--green)]" : "text-[color:var(--orange)]"
                  }
                >
                  {s.lag}
                </span>
              </td>
              <td className="py-2 pr-4">
                {s.active_ms === null ? (
                  <span className="text-[color:var(--text-muted)]">never</span>
                ) : (
                  <span className={stale ? "text-[color:var(--red)]" : "text-[color:var(--green)]"}>
                    {formatActive(s.active_ms)}
                  </span>
                )}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function formatActive(ms: number): string {
  if (ms < 1000) return `${ms}ms ago`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s ago`;
  if (ms < 3_600_000) return `${Math.round(ms / 60_000)}m ago`;
  return `${Math.round(ms / 3_600_000)}h ago`;
}
