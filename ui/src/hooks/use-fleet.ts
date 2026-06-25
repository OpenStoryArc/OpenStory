/**
 * useFleet — fetches the configured Person + their Principals from
 * `GET /api/fleet`.
 *
 * The Sidebar uses this to resolve `principal_id` (per-session) into a
 * `display_name` for the fleet group header. Returns `null` for `fleet`
 * when no `[person]` section is configured (404 from the API).
 *
 * Lightweight clone of `useSessionsList`'s shape — fetches once on mount
 * and on connection re-establish. The fleet rarely changes at runtime
 * (it's config-driven in v1), so no polling.
 */

import { useEffect, useState } from "react";
import { useConnectionStatus } from "./use-connection-status";
import { fetchFleet, type Fleet } from "@/lib/story-api";

export interface UseFleetResult {
  readonly fleet: Fleet | null;
  readonly loading: boolean;
  readonly error: string | null;
}

export function useFleet(): UseFleetResult {
  const [fleet, setFleet] = useState<Fleet | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const status = useConnectionStatus();

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    fetchFleet()
      .then((f) => {
        if (cancelled) return;
        setFleet(f);
        setLoading(false);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [status === "connected"]);

  return { fleet, loading, error };
}
