/**
 * FleetPresence — every node's latest beat, one path for everyone (P-05).
 *
 * Polls GET /api/fleet/presence every 15 s and lists each node with its
 * dot (the verdict of its last beat, or critical once stale), its age,
 * its sha, and a stale badge. The local node is in the same list from the
 * same endpoint; there is no second path for self. Reads only.
 */

import { useCallback, useEffect, useState } from "react";
import { presenceRows, type FleetPresence as FleetPresenceBody, type PresenceRow } from "@/lib/fleet-presence";
import type { HealthLevel } from "@/lib/health-verdict";

const POLL_MS = 15_000;

const COLOR: Record<HealthLevel, string> = {
  ok: "bg-green-400",
  warn: "bg-amber-400",
  critical: "bg-red-500",
};

interface Props {
  /** This device's host, to flag its own row. */
  readonly selfHost: string | null;
}

type Reading =
  | { readonly kind: "loading" }
  | { readonly kind: "error"; readonly reason: string }
  | { readonly kind: "ok"; readonly intervalSecs: number; readonly rows: readonly PresenceRow[] };

async function readPresence(selfHost: string | null): Promise<Reading> {
  try {
    const resp = await fetch("/api/fleet/presence");
    if (!resp.ok) return { kind: "error", reason: `HTTP ${resp.status}` };
    const body = (await resp.json()) as FleetPresenceBody;
    return { kind: "ok", intervalSecs: body.interval_secs, rows: presenceRows(body.nodes ?? [], selfHost) };
  } catch (e) {
    return { kind: "error", reason: e instanceof Error ? e.message : String(e) };
  }
}

export function FleetPresence({ selfHost }: Props) {
  const [reading, setReading] = useState<Reading>({ kind: "loading" });

  const refresh = useCallback(() => {
    void readPresence(selfHost).then(setReading);
  }, [selfHost]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, POLL_MS);
    return () => clearInterval(id);
  }, [refresh]);

  return (
    <div data-testid="fleet-presence" className="mb-3">
      {reading.kind === "loading" && (
        <p className="text-xs text-[color:var(--text-muted)]">Reading presence…</p>
      )}
      {reading.kind === "error" && (
        <p className="text-xs text-red-500">Presence unavailable: {reading.reason}</p>
      )}
      {reading.kind === "ok" && (
        <>
          <p className="mb-2 text-xs text-[color:var(--text-muted)]">
            Who is alive, from each node's own beat every {reading.intervalSecs} s.
          </p>
          {reading.rows.length === 0 ? (
            <p className="text-xs text-[color:var(--text-muted)]">No presence yet.</p>
          ) : (
            <ul className="divide-y divide-[color:var(--bg-surface)] rounded-lg border border-[color:var(--bg-surface)]">
              {reading.rows.map((r) => (
                <li
                  key={`${r.host}/${r.principalId}`}
                  data-testid={`presence-node-${r.host}`}
                  className="flex items-center gap-3 px-3 py-2 text-xs"
                >
                  <span
                    data-testid="presence-dot"
                    data-level={r.level}
                    title={r.findings.length ? `${r.level}: ${r.findings.join("; ")}` : r.level}
                    className={`h-2.5 w-2.5 shrink-0 rounded-full ${COLOR[r.level]}`}
                  />
                  <code className="font-semibold text-[color:var(--text)]" title={r.principalId}>
                    {r.host}
                  </code>
                  {r.isSelf && (
                    <span className="rounded bg-[color:var(--accent)]/20 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-[color:var(--accent)]">
                      self
                    </span>
                  )}
                  {r.stale && (
                    <span className="rounded bg-red-500/15 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-red-500">
                      stale
                    </span>
                  )}
                  <span className="ml-auto text-[color:var(--text-muted)]" title={r.time}>
                    {r.ageLabel}
                  </span>
                  {r.gitSha && (
                    <code className="text-[color:var(--text-muted)]" title={r.version ?? undefined}>
                      {r.gitSha}
                    </code>
                  )}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}
