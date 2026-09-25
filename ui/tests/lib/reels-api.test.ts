/**
 * /api/reels typed wrappers — the beat-ink write path.
 *
 * Marginalia used to live only in one browser's localStorage. putReelBeatInk
 * is the seam that carries a beat's strokes to the reel record itself.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import { putReelBeatInk } from "@/lib/reels-api";

const strokes = [
  { type: "path" as const, points: [{ x: 0.1, y: 0.2 }, { x: 0.3, y: 0.4 }] },
];

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("when the client puts ink for one beat of a reel", () => {
  it("should PUT the strokes to that reel's beat and resolve true on 200", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ ok: true }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const ok = await putReelBeatInk("reel-abc", 1, strokes, "2026-09-25T00:32:33Z");

    expect(ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("/api/reels/reel-abc/ink/1");
    expect(init.method).toBe("PUT");
    expect(JSON.parse(String(init.body))).toEqual({
      strokes,
      updatedAt: "2026-09-25T00:32:33Z",
    });
  });

  it("should resolve false when the server rejects, without throwing", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 404 })));
    await expect(putReelBeatInk("reel-abc", 0, [], "")).resolves.toBe(false);
  });

  it("should resolve false when the network fails, without throwing", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("offline"); }));
    await expect(putReelBeatInk("reel-abc", 0, strokes, "")).resolves.toBe(false);
  });
});
