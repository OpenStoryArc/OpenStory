/**
 * useLocalInfo — fetches /api/local-info once on mount and caches it.
 *
 * The local host/user is stable for the life of the page (changes
 * require a container restart), so we don't poll. Returns `null`
 * while loading or on error — callers should defensively handle both.
 */

import { useEffect, useState } from "react";
import { fetchLocalInfo, type LocalInfo } from "@/lib/local-info";

export function useLocalInfo(): LocalInfo | null {
  const [info, setInfo] = useState<LocalInfo | null>(null);

  useEffect(() => {
    const ctrl = new AbortController();
    fetchLocalInfo()
      .then((r) => {
        if (!ctrl.signal.aborted) setInfo(r);
      })
      .catch(() => {
        // Stay silent — the SessionHeader degrades gracefully when
        // local-info is unavailable (just doesn't render the
        // "replicated" indicator). No user-facing error needed.
      });
    return () => ctrl.abort();
  }, []);

  return info;
}
