import { describe, it, expect } from "vitest";
import {
  DRAW_PERSIST_VERSION,
  DRAW_STORAGE_KEY,
  loadPersistedScene,
  persistBlobToScene,
  savePersistedScene,
  sceneToPersistBlob,
} from "@/lib/draw-persist";
import { EMPTY_SCENE, type DrawScene } from "@/lib/draw";

describe("sceneToPersistBlob / persistBlobToScene", () => {
  it("round-trips a small scene", () => {
    const scene: DrawScene = {
      strokes: [
        { type: "circle", cx: 0.5, cy: 0.5, r: 0.1, stroke: "#000" },
        { type: "text", x: 0.5, y: 0.2, text: "hi", fill: "#111" },
      ],
      visible: true,
      label: "human",
    };
    const blob = sceneToPersistBlob(scene, { now: () => "t0" });
    expect(blob.v).toBe(DRAW_PERSIST_VERSION);
    expect(blob.savedAt).toBe("t0");
    const back = persistBlobToScene(blob);
    expect(back?.label).toBe("human");
    expect(back?.strokes).toHaveLength(2);
    expect(back?.strokes[0]).toMatchObject({ type: "circle", cx: 0.5 });
  });

  it("rejects wrong version", () => {
    expect(persistBlobToScene({ v: 99, scene: { strokes: [], visible: true } })).toBeNull();
  });

  it("truncates on save cap", () => {
    const strokes = Array.from({ length: 10 }, (_, i) => ({
      type: "line" as const,
      x1: 0,
      y1: i / 10,
      x2: 1,
      y2: i / 10,
    }));
    const blob = sceneToPersistBlob(
      { strokes, visible: true },
      { maxStrokes: 3 },
    );
    expect(blob.scene.strokes).toHaveLength(3);
  });
});

describe("loadPersistedScene / savePersistedScene", () => {
  it("saves and loads via a mock storage", () => {
    const map = new Map<string, string>();
    const storage = {
      getItem: (k: string) => map.get(k) ?? null,
      setItem: (k: string, v: string) => {
        map.set(k, v);
      },
      removeItem: (k: string) => {
        map.delete(k);
      },
    };
    const scene: DrawScene = {
      strokes: [{ type: "line", x1: 0.1, y1: 0.1, x2: 0.9, y2: 0.9, stroke: "#2f4a3e" }],
      visible: false,
      label: "note",
    };
    expect(savePersistedScene(scene, storage)).toBe(true);
    expect(map.has(DRAW_STORAGE_KEY)).toBe(true);
    const loaded = loadPersistedScene(storage);
    expect(loaded.visible).toBe(false);
    expect(loaded.label).toBe("note");
    expect(loaded.strokes).toHaveLength(1);

    // empty clears key
    expect(savePersistedScene({ ...EMPTY_SCENE }, storage)).toBe(true);
    expect(map.has(DRAW_STORAGE_KEY)).toBe(false);
    expect(loadPersistedScene(storage).strokes).toHaveLength(0);
  });
});
