/**
 * PersonRow — primary Person filter surface for the Live tab.
 *
 * Renders an "All" chip plus one `PersonChip` per distinct stamped user
 * (from `/api/users`). Click a chip → sets the sidebar's `userFilter`.
 * Click the chip a second time → clears it. Click "All" → clears.
 *
 * Hidden entirely when the universe holds 0 or 1 stamped users — a filter
 * row of one option isn't a filter, it's noise. `null` is returned in that
 * case so the parent's vertical rhythm stays clean.
 *
 * Design adapted from openstory-ui-prototype's left-rail person list,
 * compressed into a horizontal row that fits the existing sidebar width.
 * The small per-card `⌂host @user` badges added in PR #42 stay — those
 * are useful for spotting cross-user sessions while scrolling. PersonRow
 * is the *primary* filter; the badges become *secondary indicators*.
 */

import { useEffect, useMemo, useState } from "react";
import { fetchUsers, type UserSummary } from "@/lib/users-api";
import { PersonChip } from "@/components/PersonChip";

const ACTIVE_NOW_THRESHOLD_MS = 5 * 60 * 1000; // 5 min — matches sidebar staleness

export interface PersonRowProps {
  /** Currently-active user filter, or null. */
  userFilter: string | null;
  /** Setter — passed in so PersonRow doesn't own filter state. */
  onUserFilterChange: (user: string | null) => void;
  /** Optional refresh trigger: re-fetch /api/users when this changes. */
  refreshKey?: number;
}

export function PersonRow({
  userFilter,
  onUserFilterChange,
  refreshKey = 0,
}: PersonRowProps) {
  const [users, setUsers] = useState<readonly UserSummary[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const ctrl = new AbortController();
    setLoading(true);
    fetchUsers()
      .then((r) => {
        if (ctrl.signal.aborted) return;
        setUsers(r.users);
        setLoading(false);
      })
      .catch(() => {
        if (ctrl.signal.aborted) return;
        // Stay silent on error — the Users tab surfaces the message; this
        // row is a filter, not a status display.
        setUsers([]);
        setLoading(false);
      });
    return () => ctrl.abort();
  }, [refreshKey]);

  const now = Date.now();
  const sortedUsers = useMemo(() => {
    // Order: most recently active first, matching `/api/users`'s default sort.
    return [...users].sort((a, b) =>
      (b.last_active ?? "").localeCompare(a.last_active ?? ""),
    );
  }, [users]);

  // Don't render anything until we know whether there's >1 user to show.
  if (loading) return null;
  if (sortedUsers.length <= 1) return null;

  return (
    <div
      className="px-2 py-2 border-b border-[#2f3348] flex items-center gap-1.5 overflow-x-auto"
      data-testid="person-row"
    >
      <button
        type="button"
        onClick={() => onUserFilterChange(null)}
        data-testid="person-chip-all"
        data-selected={userFilter === null}
        className={`shrink-0 px-3 py-1.5 rounded-lg text-[12px] font-medium transition-colors ${
          userFilter === null
            ? "bg-[#7aa2f7] text-[#1a1b26]"
            : "text-[#565f89] hover:text-[#c0caf5] hover:bg-[#24283b]"
        }`}
      >
        All ({sortedUsers.length})
      </button>
      {sortedUsers.map((u) => {
        const lastActiveMs = u.last_active
          ? new Date(u.last_active).getTime()
          : 0;
        const isActiveNow =
          lastActiveMs > 0 && now - lastActiveMs < ACTIVE_NOW_THRESHOLD_MS;
        return (
          <PersonChip
            key={u.user}
            user={u.user}
            sessionCount={u.session_count}
            selected={userFilter === u.user}
            isActiveNow={isActiveNow}
            onClick={() =>
              onUserFilterChange(userFilter === u.user ? null : u.user)
            }
          />
        );
      })}
    </div>
  );
}
