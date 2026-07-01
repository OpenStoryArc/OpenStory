/** useRecents — frecency-ranked recently-viewed session ids, persisted locally.
 *  Pure ranking lives in lib/recents.ts; this is the React/localStorage edge. */

import { useCallback, useMemo, useState } from "react";
import { loadRecents, recordVisit, rankRecents, saveRecents, type RecentsState } from "@/lib/recents";

export function useRecents(): { recentIds: string[]; record: (id: string) => void } {
  const [state, setState] = useState<RecentsState>(() => loadRecents());

  const record = useCallback((id: string) => {
    setState((prev) => {
      const next = recordVisit(prev, id, Date.now());
      saveRecents(next);
      return next;
    });
  }, []);

  const recentIds = useMemo(() => rankRecents(state, Date.now()), [state]);
  return { recentIds, record };
}
