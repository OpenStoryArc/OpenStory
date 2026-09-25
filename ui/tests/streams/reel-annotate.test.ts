/**
 * Beat-ink stream — write-through to the reel record.
 *
 * The stream keeps its localStorage cache (offline, instant), but every ink
 * change on a beat also lands on the reel via PUT /api/reels/{id}/ink/{beat},
 * and a fetched reel hydrates the cache. The server copy is the one that
 * follows the reel to other devices, exports, and agents.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  appendActiveBeatStrokes,
  clearActiveBeatInk,
  clearAllBeatInk,
  commitBeatInkIntent,
  getBeatInkStore,
  hydrateFromReel,
  setActiveBeatKey,
} from "@/streams/reel-annotate";
import { getBeatInk } from "@/lib/reel-annotate";

const stroke = { type: "path" as const, points: [{ x: 0.1, y: 0.2 }, { x: 0.3, y: 0.4 }] };

function okFetch() {
  return vi.fn(async () => new Response(JSON.stringify({ ok: true }), { status: 200 }));
}

function lastPut(fetchMock: ReturnType<typeof okFetch>) {
  const calls = fetchMock.mock.calls as unknown as [string, RequestInit][];
  const [url, init] = calls[calls.length - 1]!;
  return { url, method: init.method, body: JSON.parse(String(init.body)) as { strokes: unknown[]; updatedAt: string } };
}

beforeEach(() => {
  clearAllBeatInk();
  setActiveBeatKey(null);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("when the human draws on the active beat", () => {
  it("should append locally and PUT the beat's full stroke list to the reel", async () => {
    const fetchMock = okFetch();
    vi.stubGlobal("fetch", fetchMock);
    setActiveBeatKey({ reelId: "reel-a", beatIndex: 1 });

    appendActiveBeatStrokes([stroke]);
    appendActiveBeatStrokes([stroke]);

    expect(getBeatInk(getBeatInkStore(), { reelId: "reel-a", beatIndex: 1 }).strokes).toHaveLength(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    const put = lastPut(fetchMock);
    expect(put.url).toBe("/api/reels/reel-a/ink/1");
    expect(put.method).toBe("PUT");
    expect(put.body.strokes).toHaveLength(2);
    expect(put.body.updatedAt).not.toBe("");
  });

  it("should keep the local copy when the server is unreachable", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("offline"); }));
    setActiveBeatKey({ reelId: "reel-a", beatIndex: 1 });

    appendActiveBeatStrokes([stroke]);
    await Promise.resolve();

    expect(getBeatInk(getBeatInkStore(), { reelId: "reel-a", beatIndex: 1 }).strokes).toHaveLength(1);
  });
});

describe("when the human clears the active beat", () => {
  it("should PUT an empty stroke list so the reel forgets that beat", () => {
    const fetchMock = okFetch();
    vi.stubGlobal("fetch", fetchMock);
    setActiveBeatKey({ reelId: "reel-a", beatIndex: 2 });
    appendActiveBeatStrokes([stroke]);

    clearActiveBeatInk();

    const put = lastPut(fetchMock);
    expect(put.url).toBe("/api/reels/reel-a/ink/2");
    expect(put.body.strokes).toEqual([]);
  });
});

describe("when an agent commits ink to a specific slide", () => {
  it("should PUT that slide's strokes without needing player focus", () => {
    const fetchMock = okFetch();
    vi.stubGlobal("fetch", fetchMock);

    commitBeatInkIntent({ reelId: "reel-b", beatIndex: 4, strokes: [stroke], mode: "replace" });

    const put = lastPut(fetchMock);
    expect(put.url).toBe("/api/reels/reel-b/ink/4");
    expect(put.body.strokes).toHaveLength(1);
  });
});

describe("when a reel is fetched from the server", () => {
  it("should hydrate the cache from the reel's beatInk without writing back", () => {
    const fetchMock = okFetch();
    vi.stubGlobal("fetch", fetchMock);

    hydrateFromReel({
      id: "reel-c",
      title: "t",
      created: "",
      author: "a",
      stops: [],
      beatInk: { "1": { strokes: [stroke, stroke], updatedAt: "2026-09-25T00:32:33Z" } },
    });

    expect(getBeatInk(getBeatInkStore(), { reelId: "reel-c", beatIndex: 1 }).strokes).toHaveLength(2);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("when a reel is fetched that the server has never inked but this browser has", () => {
  it("should migrate the local ink up to the reel instead of wiping it", () => {
    const fetchMock = okFetch();
    vi.stubGlobal("fetch", fetchMock);
    setActiveBeatKey({ reelId: "reel-d", beatIndex: 1 });
    appendActiveBeatStrokes([stroke, stroke]);
    fetchMock.mockClear();

    hydrateFromReel({ id: "reel-d", title: "t", created: "", author: "a", stops: [] });

    expect(getBeatInk(getBeatInkStore(), { reelId: "reel-d", beatIndex: 1 }).strokes).toHaveLength(2);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const put = lastPut(fetchMock);
    expect(put.url).toBe("/api/reels/reel-d/ink/1");
    expect(put.body.strokes).toHaveLength(2);
  });

  it("should still let a server copy with ink win over stale local ink", () => {
    const fetchMock = okFetch();
    vi.stubGlobal("fetch", fetchMock);
    setActiveBeatKey({ reelId: "reel-e", beatIndex: 3 });
    appendActiveBeatStrokes([stroke]);
    fetchMock.mockClear();

    hydrateFromReel({
      id: "reel-e", title: "t", created: "", author: "a", stops: [],
      beatInk: { "1": { strokes: [stroke], updatedAt: "" } },
    });

    expect(getBeatInk(getBeatInkStore(), { reelId: "reel-e", beatIndex: 3 }).strokes).toHaveLength(0);
    expect(getBeatInk(getBeatInkStore(), { reelId: "reel-e", beatIndex: 1 }).strokes).toHaveLength(1);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
