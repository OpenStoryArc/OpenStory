/**
 * The shared read-model cache: a session's records are fetched ONCE and
 * every surface (Story header, Explore viz, timeline) shares the promise.
 *
 * This is the "data loads once, from a shared read model" north-star as a
 * pure, testable unit — a keyed promise cache with honest failure handling
 * (rejections are evicted so the next consumer retries, never a cached error).
 */

import { describe, it, expect, vi } from "vitest";
import { scenario, scenarioAsync } from "../bdd";
import { createKeyedPromiseCache, liveInvalidationKey } from "@/lib/record-cache";
import type { WsMessage } from "@/types/websocket";

function makeFetcher(payload: (id: string) => unknown = (id) => [`records-of-${id}`]) {
  return vi.fn((id: string) => Promise.resolve(payload(id)));
}

describe("when two consumers ask for the same session's records", () => {
  it("should call the fetcher once and hand both the same promise", () =>
    scenarioAsync(
      () => {
        const fetcher = makeFetcher();
        const cache = createKeyedPromiseCache(fetcher);
        return { fetcher, cache };
      },
      async ({ fetcher, cache }) => {
        const a = cache.get("s1");
        const b = cache.get("s1");
        return { fetcher, samePromise: a === b, results: await Promise.all([a, b]) };
      },
      ({ fetcher, samePromise, results }) => {
        expect(fetcher).toHaveBeenCalledTimes(1);
        expect(samePromise).toBe(true);
        expect(results[0]).toEqual(["records-of-s1"]);
        expect(results[0]).toBe(results[1]);
      },
    ));
});

describe("when consumers ask for different sessions", () => {
  it("should fetch each session independently", () =>
    scenarioAsync(
      () => {
        const fetcher = makeFetcher();
        const cache = createKeyedPromiseCache(fetcher);
        return { fetcher, cache };
      },
      async ({ fetcher, cache }) => ({
        fetcher,
        results: await Promise.all([cache.get("s1"), cache.get("s2")]),
      }),
      ({ fetcher, results }) => {
        expect(fetcher).toHaveBeenCalledTimes(2);
        expect(fetcher).toHaveBeenCalledWith("s1");
        expect(fetcher).toHaveBeenCalledWith("s2");
        expect(results).toEqual([["records-of-s1"], ["records-of-s2"]]);
      },
    ));
});

describe("when a fetch fails", () => {
  it("should evict the entry so the next consumer retries instead of receiving a cached rejection", () =>
    scenarioAsync(
      () => {
        let calls = 0;
        const fetcher = vi.fn((id: string) => {
          calls += 1;
          return calls === 1
            ? Promise.reject(new Error("network down"))
            : Promise.resolve([`records-of-${id}`]);
        });
        const cache = createKeyedPromiseCache(fetcher);
        return { fetcher, cache };
      },
      async ({ fetcher, cache }) => {
        const firstError = await cache.get("s1").then(
          () => null,
          (e: Error) => e.message,
        );
        // Rejection settles the eviction in the same microtask chain; a
        // fresh get() must trigger a brand-new fetch.
        const retried = await cache.get("s1");
        return { fetcher, firstError, retried };
      },
      ({ fetcher, firstError, retried }) => {
        expect(firstError).toBe("network down");
        expect(retried).toEqual(["records-of-s1"]);
        expect(fetcher).toHaveBeenCalledTimes(2);
      },
    ));
});

describe("when a session is invalidated (e.g. it is live and grew)", () => {
  it("should refetch on the next get while other sessions stay cached", () =>
    scenarioAsync(
      () => {
        const fetcher = makeFetcher();
        const cache = createKeyedPromiseCache(fetcher);
        return { fetcher, cache };
      },
      async ({ fetcher, cache }) => {
        await Promise.all([cache.get("s1"), cache.get("s2")]);
        cache.invalidate("s1");
        await Promise.all([cache.get("s1"), cache.get("s2")]);
        return fetcher;
      },
      (fetcher) => {
        const calls = fetcher.mock.calls.map(([id]) => id);
        expect(calls).toEqual(["s1", "s2", "s1"]);
      },
    ));
});

describe("when the cache is cleared", () => {
  it("should forget every session", () =>
    scenarioAsync(
      () => {
        const fetcher = makeFetcher();
        const cache = createKeyedPromiseCache(fetcher);
        return { fetcher, cache };
      },
      async ({ fetcher, cache }) => {
        await Promise.all([cache.get("s1"), cache.get("s2")]);
        cache.clear();
        await Promise.all([cache.get("s1"), cache.get("s2")]);
        return fetcher;
      },
      (fetcher) => expect(fetcher).toHaveBeenCalledTimes(4),
    ));
});

describe("when a live WebSocket message arrives", () => {
  it("should name the session to invalidate only for new-record broadcasts", () =>
    scenario(
      () => ({
        grew: { kind: "view_records", session_id: "s-live", view_records: [] } as WsMessage,
        unrelated: { kind: "session_list", sessions: [] } as WsMessage,
      }),
      ({ grew, unrelated }) => ({
        grewKey: liveInvalidationKey(grew),
        unrelatedKey: liveInvalidationKey(unrelated),
      }),
      ({ grewKey, unrelatedKey }) => {
        expect(grewKey).toBe("s-live");
        expect(unrelatedKey).toBeNull();
      },
    ));
});

describe("when asking whether a session is already cached", () => {
  it("should report presence without triggering a fetch", () =>
    scenario(
      () => {
        const fetcher = makeFetcher();
        const cache = createKeyedPromiseCache(fetcher);
        cache.get("s1");
        return { fetcher, cache };
      },
      ({ fetcher, cache }) => ({ fetcher, hasS1: cache.has("s1"), hasS2: cache.has("s2") }),
      ({ fetcher, hasS1, hasS2 }) => {
        expect(hasS1).toBe(true);
        expect(hasS2).toBe(false);
        expect(fetcher).toHaveBeenCalledTimes(1);
      },
    ));
});
