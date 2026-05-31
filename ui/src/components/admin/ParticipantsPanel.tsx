/**
 * ParticipantsPanel — manage the role directory from the UI.
 *
 * Lists everyone in the local `EmbeddedRoleDirectory`, lets an Admin
 * grant/revoke roles. First Admin must be bootstrapped via
 * `open-story grant-role` from the CLI (chicken-and-egg: every admin
 * endpoint requires an existing Admin to authorize).
 */

import { useCallback, useEffect, useState } from "react";
import {
  deleteParticipant,
  fetchParticipants,
  upsertParticipant,
  type Participant,
  type Role,
} from "@/lib/admin-api";

const ROLES: readonly Role[] = ["observer", "contributor", "admin"];

export function ParticipantsPanel() {
  const [participants, setParticipants] = useState<readonly Participant[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<Set<string>>(new Set());

  // Add-form state
  const [newPrincipalId, setNewPrincipalId] = useState("");
  const [newPersonId, setNewPersonId] = useState("");
  const [newRole, setNewRole] = useState<Role>("observer");

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

  const markBusy = (id: string, on: boolean) =>
    setBusy((prev) => {
      const next = new Set(prev);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });

  const handleGrant = async () => {
    if (!newPrincipalId.trim() || !newPersonId.trim()) {
      setError("principal_id and person_id are required");
      return;
    }
    markBusy("__new__", true);
    try {
      await upsertParticipant(newPrincipalId.trim(), newPersonId.trim(), newRole);
      setNewPrincipalId("");
      setNewPersonId("");
      setNewRole("observer");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      markBusy("__new__", false);
    }
  };

  const handleChangeRole = async (p: Participant, role: Role) => {
    markBusy(p.principal_id, true);
    try {
      await upsertParticipant(p.principal_id, p.person_id, role);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      markBusy(p.principal_id, false);
    }
  };

  const handleRevoke = async (p: Participant) => {
    if (!window.confirm(`Revoke role for ${p.principal_id}?`)) return;
    markBusy(p.principal_id, true);
    try {
      await deleteParticipant(p.principal_id);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      markBusy(p.principal_id, false);
    }
  };

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
              <th className="py-2 pr-4"></th>
            </tr>
          </thead>
          <tbody>
            {participants.map((p) => {
              const isBusy = busy.has(p.principal_id);
              return (
                <tr key={p.principal_id} className="border-b border-[#16161e]">
                  <td className="py-2 pr-4 font-mono text-[#c0caf5]">{p.principal_id}</td>
                  <td className="py-2 pr-4 text-[#bb9af7]">{p.person_id}</td>
                  <td className="py-2 pr-4">
                    <select
                      value={p.role}
                      onChange={(e) => handleChangeRole(p, e.target.value as Role)}
                      disabled={isBusy}
                      className="rounded bg-[#16161e] border border-[#24283b] text-[#c0caf5] text-xs px-2 py-1"
                      data-testid={`role-select-${p.principal_id}`}
                    >
                      {ROLES.map((r) => (
                        <option key={r} value={r}>{r}</option>
                      ))}
                    </select>
                  </td>
                  <td className="py-2 pr-4 text-xs text-[#565f89]">
                    {p.created_at.slice(0, 16)}
                  </td>
                  <td className="py-2 pr-4">
                    <button
                      type="button"
                      onClick={() => handleRevoke(p)}
                      disabled={isBusy}
                      className="rounded px-2 py-1 text-xs bg-[#f7768e]/20 text-[#f7768e] hover:bg-[#f7768e]/30 cursor-pointer"
                      data-testid={`revoke-${p.principal_id}`}
                    >
                      revoke
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}

      <div className="mt-4 pt-4 border-t border-[#24283b]">
        <h4 className="text-xs font-medium text-[#565f89] uppercase tracking-wider mb-2">
          Grant role
        </h4>
        <div className="flex gap-2 items-end">
          <div className="flex-1">
            <label className="text-xs text-[#565f89] block mb-1">Principal id</label>
            <input
              type="text"
              value={newPrincipalId}
              onChange={(e) => setNewPrincipalId(e.target.value)}
              placeholder="max-laptop"
              className="w-full rounded bg-[#16161e] border border-[#24283b] text-[#c0caf5] text-sm px-2 py-1 font-mono"
              data-testid="new-principal-id"
            />
          </div>
          <div className="flex-1">
            <label className="text-xs text-[#565f89] block mb-1">Person id</label>
            <input
              type="text"
              value={newPersonId}
              onChange={(e) => setNewPersonId(e.target.value)}
              placeholder="max"
              className="w-full rounded bg-[#16161e] border border-[#24283b] text-[#c0caf5] text-sm px-2 py-1 font-mono"
              data-testid="new-person-id"
            />
          </div>
          <div>
            <label className="text-xs text-[#565f89] block mb-1">Role</label>
            <select
              value={newRole}
              onChange={(e) => setNewRole(e.target.value as Role)}
              className="rounded bg-[#16161e] border border-[#24283b] text-[#c0caf5] text-sm px-2 py-1"
              data-testid="new-role"
            >
              {ROLES.map((r) => (
                <option key={r} value={r}>{r}</option>
              ))}
            </select>
          </div>
          <button
            type="button"
            onClick={handleGrant}
            disabled={busy.has("__new__")}
            className="rounded px-3 py-1 text-sm font-medium bg-[#7aa2f7]/20 text-[#7aa2f7] hover:bg-[#7aa2f7]/30 cursor-pointer"
            data-testid="grant-button"
          >
            grant
          </button>
        </div>
      </div>
    </div>
  );
}
