/**
 * SharePolicyTable — per-session share/private toggle (Admin v0).
 *
 * The operator's authority surface for Phase 4 edge sovereignty. v0
 * shows sessions whose `host` matches *this* device — you can only set
 * policy on YOUR sessions (a leaf can't decide a peer's policy). Sessions
 * default to `shared`; only explicit `private` rows live in the DB.
 *
 * v0 does the optimistic update + revert-on-error pattern: click toggles
 * the row immediately, server PUT fires; on failure the row snaps back.
 * Live invalidation via WS lands in a follow-up commit.
 */

import { useEffect, useState } from "react";
import {
  fetchSharePolicies,
  setSharePolicy,
  type SharePolicyMode,
} from "@/lib/admin-api";

interface SessionLite {
  readonly session_id: string;
  readonly host: string | null;
  readonly label: string | null;
  readonly project_name: string | null;
  readonly last_event: string | null;
}

interface Props {
  /** This device's host — sessions whose host !== selfHost are read-only. */
  selfHost: string;
}

export function SharePolicyTable({ selfHost }: Props) {
  const [sessions, setSessions] = useState<SessionLite[] | null>(null);
  const [policies, setPolicies] = useState<Map<string, SharePolicyMode>>(new Map());
  const [error, setError] = useState<string | null>(null);
  const [savingIds, setSavingIds] = useState<Set<string>>(new Set());

  useEffect(() => {
    const ctrl = new AbortController();
    Promise.all([
      fetch("/api/sessions", { signal: ctrl.signal }).then((r) => r.json()),
      fetchSharePolicies(ctrl.signal),
    ])
      .then(([s, p]) => {
        if (ctrl.signal.aborted) return;
        setSessions(
          (s.sessions ?? []).map((row: Record<string, unknown>) => ({
            session_id: String(row.session_id),
            host: row.host as string | null,
            label: row.label as string | null,
            project_name: row.project_name as string | null,
            last_event: row.last_event as string | null,
          })),
        );
        const map = new Map<string, SharePolicyMode>();
        for (const row of p.policies) map.set(row.session_id, row.mode);
        setPolicies(map);
        setError(null);
      })
      .catch((err) => {
        if (ctrl.signal.aborted) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => ctrl.abort();
  }, []);

  const handleToggle = async (sessionId: string, current: SharePolicyMode) => {
    const next: SharePolicyMode = current === "private" ? "shared" : "private";
    // Optimistic
    setPolicies((prev) => {
      const m = new Map(prev);
      m.set(sessionId, next);
      return m;
    });
    setSavingIds((prev) => new Set(prev).add(sessionId));
    try {
      await setSharePolicy(sessionId, next);
    } catch (err) {
      // Revert
      setPolicies((prev) => {
        const m = new Map(prev);
        m.set(sessionId, current);
        return m;
      });
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingIds((prev) => {
        const next = new Set(prev);
        next.delete(sessionId);
        return next;
      });
    }
  };

  if (!sessions) {
    return <p className="text-sm text-[#565f89]">Loading sessions…</p>;
  }

  const ownSessions = sessions.filter((s) => s.host === selfHost);

  return (
    <div>
      {error && (
        <div className="mb-3 rounded border border-red-900 bg-red-950/30 px-3 py-2 text-sm text-red-300">
          {error}
        </div>
      )}

      {ownSessions.length === 0 ? (
        <p className="text-sm text-[#565f89]">
          No sessions originate on this device (<code>{selfHost}</code>).
          Sessions from other hosts can be observed but not toggled here —
          their policy is the originating device's authority.
        </p>
      ) : (
        <table className="w-full text-sm" data-testid="share-policy-table">
          <thead>
            <tr className="text-left text-xs uppercase tracking-wider text-[#565f89] border-b border-[#24283b]">
              <th className="py-2 pr-4">Session</th>
              <th className="py-2 pr-4">Project</th>
              <th className="py-2 pr-4">Last event</th>
              <th className="py-2 pr-4">Share policy</th>
            </tr>
          </thead>
          <tbody>
            {ownSessions.map((s) => {
              const mode = policies.get(s.session_id) ?? "shared";
              const saving = savingIds.has(s.session_id);
              return (
                <tr key={s.session_id} className="border-b border-[#16161e]">
                  <td className="py-2 pr-4">
                    <div className="text-[#c0caf5] truncate max-w-[280px]">
                      {s.label ?? <em className="text-[#565f89]">(unlabeled)</em>}
                    </div>
                    <code className="text-xs text-[#565f89]">{s.session_id.slice(0, 8)}</code>
                  </td>
                  <td className="py-2 pr-4 text-[#9ece6a]">
                    {s.project_name ?? <em className="text-[#565f89]">—</em>}
                  </td>
                  <td className="py-2 pr-4 text-xs text-[#565f89]">
                    {s.last_event?.slice(0, 16) ?? "—"}
                  </td>
                  <td className="py-2 pr-4">
                    <button
                      type="button"
                      onClick={() => handleToggle(s.session_id, mode)}
                      disabled={saving}
                      className={`rounded px-2 py-1 text-xs font-medium transition-colors ${
                        mode === "private"
                          ? "bg-[#f7768e]/20 text-[#f7768e] hover:bg-[#f7768e]/30"
                          : "bg-[#9ece6a]/20 text-[#9ece6a] hover:bg-[#9ece6a]/30"
                      } ${saving ? "opacity-50 cursor-wait" : "cursor-pointer"}`}
                      data-testid={`share-policy-${s.session_id}`}
                      aria-label={`Toggle share policy for ${s.label ?? s.session_id}`}
                    >
                      {mode === "private" ? "private" : "shared"}
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}
