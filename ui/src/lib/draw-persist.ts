/**
 * Client-local persistence for the attention canvas (draw$).
 * Never writes to observed history / EventStore — localStorage only.
 */

import {
  EMPTY_SCENE,
  normalizeStrokes,
  type DrawScene,
  type DrawStroke,
} from "@/lib/draw";

export const DRAW_STORAGE_KEY = "openstory.draw.scene.v1";
export const DRAW_PERSIST_VERSION = 1;

/** Soft cap so a long play session doesn't blow localStorage. */
export const DRAW_PERSIST_MAX_STROKES = 400;

export type DrawPersistBlob = {
  readonly v: number;
  readonly scene: {
    readonly strokes: readonly unknown[];
    readonly visible: boolean;
    readonly label?: string;
  };
  readonly savedAt: string;
};

/** Pure: scene → JSON-safe blob (truncated if needed). */
export function sceneToPersistBlob(
  scene: DrawScene,
  opts?: { maxStrokes?: number; now?: () => string },
): DrawPersistBlob {
  const max = opts?.maxStrokes ?? DRAW_PERSIST_MAX_STROKES;
  const strokes = scene.strokes.slice(0, max) as DrawStroke[];
  return {
    v: DRAW_PERSIST_VERSION,
    scene: {
      strokes,
      visible: scene.visible !== false,
      ...(scene.label ? { label: scene.label } : {}),
    },
    savedAt: opts?.now?.() ?? new Date().toISOString(),
  };
}

/** Pure: untrusted storage JSON → DrawScene or null. */
export function persistBlobToScene(raw: unknown): DrawScene | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  if (o.v !== DRAW_PERSIST_VERSION) return null;
  const sc = o.scene;
  if (!sc || typeof sc !== "object") return null;
  const s = sc as Record<string, unknown>;
  const strokes = normalizeStrokes(s.strokes);
  return {
    strokes,
    visible: s.visible !== false,
    label: typeof s.label === "string" ? s.label : undefined,
  };
}

/** Read from localStorage (browser only). Empty on miss/corrupt. */
export function loadPersistedScene(
  storage: Pick<Storage, "getItem"> | null = typeof localStorage !== "undefined"
    ? localStorage
    : null,
): DrawScene {
  if (!storage) return { ...EMPTY_SCENE };
  try {
    const text = storage.getItem(DRAW_STORAGE_KEY);
    if (!text) return { ...EMPTY_SCENE };
    const parsed = JSON.parse(text) as unknown;
    return persistBlobToScene(parsed) ?? { ...EMPTY_SCENE };
  } catch {
    return { ...EMPTY_SCENE };
  }
}

/** Write scene; no-op if storage missing. Returns false on failure. */
export function savePersistedScene(
  scene: DrawScene,
  storage: Pick<Storage, "setItem" | "removeItem"> | null = typeof localStorage !==
  "undefined"
    ? localStorage
    : null,
): boolean {
  if (!storage) return false;
  try {
    if (scene.strokes.length === 0) {
      storage.removeItem(DRAW_STORAGE_KEY);
      return true;
    }
    const blob = sceneToPersistBlob(scene);
    storage.setItem(DRAW_STORAGE_KEY, JSON.stringify(blob));
    return true;
  } catch {
    return false;
  }
}
