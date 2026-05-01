/**
 * useSessionsList — fetches the canonical session list from REST.
 *
 * After the lazy-load redesign (feat/lazy-load-initial-state), the
 * WebSocket handshake no longer carries records — so the Sidebar can't
 * derive its universe of sessions from accumulated events anymore.
 * This hook does the REST fetch (`GET /api/sessions`) and refreshes
 * when the WS connection re-establishes (which is when the local truth
 * may have changed).
 *
 * The hook does NOT poll on a timer — live updates flow through
 * `enriched` WS messages, which the reducer applies to label/token
 * state. The full session list refreshes on connect/reconnect.
 */

import { useEffect, useState } from "react";
import { useConnectionStatus } from "./use-connection-status";
import type { StorySession } from "@/lib/story-api";

export interface UseSessionsListResult {
  readonly sessions: readonly StorySession[];
  readonly loading: boolean;
  readonly error: string | null;
  readonly refresh: () => void;
}

async function fetchSessions(signal: AbortSignal): Promise<StorySession[]> {
  const res = await fetch("/api/sessions", { signal });
  if (!res.ok) {
    throw new Error(`GET /api/sessions: ${res.status} ${res.statusText}`);
  }
  const data = await res.json();
  if (!data || !Array.isArray(data.sessions)) {
    throw new Error("GET /api/sessions: response missing sessions array");
  }
  return data.sessions as StorySession[];
}

export function useSessionsList(): UseSessionsListResult {
  const [sessions, setSessions] = useState<readonly StorySession[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);

  const status = useConnectionStatus();

  useEffect(() => {
    const ctrl = new AbortController();
    setLoading(true);
    setError(null);
    fetchSessions(ctrl.signal)
      .then((rows) => {
        setSessions(rows);
        setLoading(false);
      })
      .catch((err) => {
        if (ctrl.signal.aborted) return;
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });
    return () => ctrl.abort();
    // Re-fetch when connection cycles to "connected" (covers reload of
    // the local state after a server restart) or when refresh() is
    // called explicitly via tick.
  }, [tick, status === "connected"]);

  return {
    sessions,
    loading,
    error,
    refresh: () => setTick((n) => n + 1),
  };
}
