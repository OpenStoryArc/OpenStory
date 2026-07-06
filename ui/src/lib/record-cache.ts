/** The shared read-model cache — "data loads once."
 *
 *  A keyed promise cache: the first consumer to ask for a key triggers the
 *  fetch; every later consumer shares the same promise (and therefore the
 *  same eventual array). Rejections are evicted immediately so a transient
 *  failure is retried by the next consumer, never served from cache.
 *
 *  Pure logic, no I/O — the fetcher is injected. The app-level singleton
 *  that binds this to GET /api/sessions/{id}/records lives in
 *  hooks/use-session-records.ts, at the side-effect boundary.
 */

export interface KeyedPromiseCache<T> {
  /** The cached promise for `key`, fetching it first if absent. */
  get(key: string): Promise<T>;
  /** True when `key` has a cached (or in-flight) promise. No fetch. */
  has(key: string): boolean;
  /** Drop one key so the next get() refetches (e.g. a live session grew). */
  invalidate(key: string): void;
  /** Drop everything. */
  clear(): void;
}

import type { WsMessage } from "@/types/websocket";

/** The session a live broadcast invalidates, or null when the message
 *  doesn't mean "this session's records grew". Keeps the cache honest for
 *  live sessions without polling — push-driven staleness. */
export function liveInvalidationKey(msg: WsMessage): string | null {
  return msg.kind === "view_records" ? msg.session_id : null;
}

export function createKeyedPromiseCache<T>(
  fetcher: (key: string) => Promise<T>,
): KeyedPromiseCache<T> {
  const entries = new Map<string, Promise<T>>();

  return {
    get(key) {
      const hit = entries.get(key);
      if (hit) return hit;
      const promise = fetcher(key);
      entries.set(key, promise);
      // Evict on failure — but only if this promise is still the resident
      // entry (an invalidate+refetch may have replaced it meanwhile).
      promise.catch(() => {
        if (entries.get(key) === promise) entries.delete(key);
      });
      return promise;
    },
    has: (key) => entries.has(key),
    invalidate(key) {
      entries.delete(key);
    },
    clear() {
      entries.clear();
    },
  };
}
