/**
 * ParticipantsPanel — read-only view of the role directory.
 *
 * Lists everyone in the local `EmbeddedRoleDirectory` with their role.
 * Granting and revoking roles is intentionally NOT wired into the UI:
 * the foundation exists in the backend (PUT/DELETE /api/admin/participants),
 * but the model isn't hardened end-to-end, so mutations stay on the CLI
 * (`open-story grant-role`). This panel observes; it never mutates.
 */

import { useCallback, useEffect, useState } from "react";
import { fetchParticipants, type Participant } from "@/lib/admin-api";

export function ParticipantsPanel() {
  const [participants, setParticipants] = useState<readonly Participant[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (signal?: AbortSignal) => {
    try {
      const ps = await fetchParticipants(signal);
      setParticipants(ps);
      setError(null);
    } catch (err) {
      if (signal?.aborted) return;
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    const ctrl = new AbortController();
    refresh(ctrl.signal);
    return () => ctrl.abort();
  }, [refresh]);

  if (participants === null) {
    return <p className="text-sm text-[#565f89]">Loading participants…</p>;
  }

  return (
    <div data-testid="participants-panel">
      {error && (
        <div className="mb-3 rounded border border-red-900 bg-red-950/30 px-3 py-2 text-sm text-red-300">
          {error}
        </div>
      )}

      {participants.length === 0 ? (
        <p className="text-sm text-[#565f89]">
          No participants yet. Bootstrap the first Admin from the CLI:
          <br />
          <code className="text-[#7aa2f7]">
            open-story grant-role --principal-id YOUR_ID --person-id YOUR_PERSON --role admin
          </code>
        </p>
      ) : (
        <table className="w-full text-sm" data-testid="participants-table">
          <thead>
            <tr className="text-left text-xs uppercase tracking-wider text-[#565f89] border-b border-[#24283b]">
              <th className="py-2 pr-4">Principal</th>
              <th className="py-2 pr-4">Person</th>
              <th className="py-2 pr-4">Role</th>
              <th className="py-2 pr-4">Granted</th>
            </tr>
          </thead>
          <tbody>
            {participants.map((p) => (
              <tr key={p.principal_id} className="border-b border-[#16161e]">
                <td className="py-2 pr-4 font-mono text-[#c0caf5]">{p.principal_id}</td>
                <td className="py-2 pr-4 text-[#bb9af7]">{p.person_id}</td>
                <td className="py-2 pr-4">
                  <span
                    className="inline-block rounded bg-[#16161e] border border-[#24283b] text-[#c0caf5] text-xs px-2 py-1"
                    data-testid={`role-${p.principal_id}`}
                  >
                    {p.role}
                  </span>
                </td>
                <td className="py-2 pr-4 text-xs text-[#565f89]">
                  {p.created_at.slice(0, 16)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <p className="mt-4 pt-4 border-t border-[#24283b] text-xs text-[#565f89]">
        Roles are managed from the CLI
        (<code className="text-[#7aa2f7]">open-story grant-role</code>), not from
        this read-only view.
      </p>
    </div>
  );
}
