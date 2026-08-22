/**
 * Beat-scoped marginalia for reels.
 *
 * A reel is an ordered list of *beats* (slides). Ink is 1:1 with a beat —
 * not a global pen floating across the app. Coordinates are unit stage space
 * (0..1), same language as diagram beats / agent pen.
 *
 * Never observed history (events.*). Client-local ui.* only for v1.
 */

import {
  EMPTY_SCENE,
  normalizeStrokes,
  type DrawStroke,
} from "@/lib/draw";

export const REEL_ANNOTATE_STORAGE_KEY = "openstory.reel.beat-ink.v1";
export const REEL_ANNOTATE_VERSION = 1;

/** Stable identity for one slide in a reel (v1: index; later optional beatId). */
export type BeatKey = {
  readonly reelId: string;
  readonly beatIndex: number;
};

export type BeatInk = {
  readonly reelId: string;
  readonly beatIndex: number;
  readonly strokes: readonly DrawStroke[];
  readonly updatedAt: string;
};

export type BeatInkStore = {
  readonly v: number;
  readonly byKey: Readonly<Record<string, BeatInk>>;
};

export function beatKeyString(key: BeatKey): string {
  return `${key.reelId}:${key.beatIndex}`;
}

export function parseBeatKey(s: string): BeatKey | null {
  const i = s.lastIndexOf(":");
  if (i <= 0) return null;
  const reelId = s.slice(0, i);
  const beatIndex = Number(s.slice(i + 1));
  if (!reelId || !Number.isInteger(beatIndex) || beatIndex < 0) return null;
  return { reelId, beatIndex };
}

export function emptyBeatInkStore(): BeatInkStore {
  return { v: REEL_ANNOTATE_VERSION, byKey: {} };
}

/** Pure: read ink for one beat (empty strokes if none). */
export function getBeatInk(store: BeatInkStore, key: BeatKey): BeatInk {
  const hit = store.byKey[beatKeyString(key)];
  if (hit) return hit;
  return {
    reelId: key.reelId,
    beatIndex: key.beatIndex,
    strokes: [],
    updatedAt: "",
  };
}

/** Pure: replace strokes for a beat. */
export function setBeatInk(
  store: BeatInkStore,
  key: BeatKey,
  strokes: readonly DrawStroke[],
  opts?: { now?: () => string },
): BeatInkStore {
  const k = beatKeyString(key);
  const updatedAt = opts?.now?.() ?? new Date().toISOString();
  if (strokes.length === 0) {
    const { [k]: _drop, ...rest } = store.byKey;
    void _drop;
    return { v: REEL_ANNOTATE_VERSION, byKey: rest };
  }
  return {
    v: REEL_ANNOTATE_VERSION,
    byKey: {
      ...store.byKey,
      [k]: {
        reelId: key.reelId,
        beatIndex: key.beatIndex,
        strokes: [...strokes],
        updatedAt,
      },
    },
  };
}

/** Pure: append strokes to a beat (marginalia accumulates on that slide only). */
export function appendBeatInk(
  store: BeatInkStore,
  key: BeatKey,
  more: readonly DrawStroke[],
  opts?: { now?: () => string },
): BeatInkStore {
  if (more.length === 0) return store;
  const cur = getBeatInk(store, key);
  return setBeatInk(store, key, [...cur.strokes, ...more], opts);
}

/**
 * Pure: agent/user parity intent for one slide.
 * - clear + strokes → replace (or empty if no strokes)
 * - append (default) → concat
 * - clear only → empty that slide
 */
export function applyBeatInkIntent(
  store: BeatInkStore,
  intent: {
    readonly reelId: string;
    readonly beatIndex: number;
    readonly clear?: boolean;
    readonly strokes?: readonly DrawStroke[];
    readonly mode?: "append" | "replace";
  },
  opts?: { now?: () => string },
): BeatInkStore {
  const key: BeatKey = { reelId: intent.reelId, beatIndex: intent.beatIndex };
  const strokes = intent.strokes ?? [];
  const replace = intent.clear === true || intent.mode === "replace";
  if (replace) {
    return setBeatInk(store, key, strokes, opts);
  }
  return appendBeatInk(store, key, strokes, opts);
}

/** Pure: inventory of ink across a reel (for review / usability matrix). */
export function summarizeReelInk(
  store: BeatInkStore,
  reelId: string,
): { beatIndex: number; strokeCount: number; empty: boolean }[] {
  const rows: { beatIndex: number; strokeCount: number; empty: boolean }[] = [];
  for (const [k, ink] of Object.entries(store.byKey)) {
    if (ink.reelId !== reelId && !k.startsWith(`${reelId}:`)) continue;
    if (ink.reelId !== reelId) continue;
    rows.push({
      beatIndex: ink.beatIndex,
      strokeCount: ink.strokes.length,
      empty: ink.strokes.length === 0,
    });
  }
  return rows.sort((a, b) => a.beatIndex - b.beatIndex);
}

/** Pure: clear one beat's ink. */
export function clearBeatInk(store: BeatInkStore, key: BeatKey): BeatInkStore {
  return setBeatInk(store, key, []);
}

/** Sanitize untrusted storage JSON. */
export function normalizeBeatInkStore(raw: unknown): BeatInkStore {
  if (!raw || typeof raw !== "object") return emptyBeatInkStore();
  const o = raw as Record<string, unknown>;
  if (o.v !== REEL_ANNOTATE_VERSION) return emptyBeatInkStore();
  const by = o.byKey;
  if (!by || typeof by !== "object") return emptyBeatInkStore();
  const out: Record<string, BeatInk> = {};
  for (const [k, v] of Object.entries(by as Record<string, unknown>)) {
    const key = parseBeatKey(k);
    if (!key || !v || typeof v !== "object") continue;
    const row = v as Record<string, unknown>;
    const strokes = normalizeStrokes(row.strokes);
    out[k] = {
      reelId: key.reelId,
      beatIndex: key.beatIndex,
      strokes,
      updatedAt: typeof row.updatedAt === "string" ? row.updatedAt : "",
    };
  }
  return { v: REEL_ANNOTATE_VERSION, byKey: out };
}

export function loadBeatInkStore(
  storage: Pick<Storage, "getItem"> | null = typeof localStorage !== "undefined"
    ? localStorage
    : null,
): BeatInkStore {
  if (!storage) return emptyBeatInkStore();
  try {
    const text = storage.getItem(REEL_ANNOTATE_STORAGE_KEY);
    if (!text) return emptyBeatInkStore();
    return normalizeBeatInkStore(JSON.parse(text) as unknown);
  } catch {
    return emptyBeatInkStore();
  }
}

export function saveBeatInkStore(
  store: BeatInkStore,
  storage: Pick<Storage, "setItem" | "removeItem"> | null = typeof localStorage !==
  "undefined"
    ? localStorage
    : null,
): boolean {
  if (!storage) return false;
  try {
    if (Object.keys(store.byKey).length === 0) {
      storage.removeItem(REEL_ANNOTATE_STORAGE_KEY);
      return true;
    }
    storage.setItem(REEL_ANNOTATE_STORAGE_KEY, JSON.stringify(store));
    return true;
  } catch {
    return false;
  }
}

/** Wire snapshot for ui-state / pen eyes style reporting. */
export function beatInkToWire(ink: BeatInk, opts?: { interactive?: boolean }) {
  return {
    reelId: ink.reelId,
    beatIndex: ink.beatIndex,
    stroke_count: ink.strokes.length,
    empty: ink.strokes.length === 0,
    interactive: opts?.interactive === true,
    updatedAt: ink.updatedAt,
    // light sample for agents — not full geometry dump
    kinds: ink.strokes.reduce(
      (acc, s) => {
        acc[s.type] = (acc[s.type] ?? 0) + 1;
        return acc;
      },
      {} as Record<string, number>,
    ),
  };
}

/** Empty scene helper for rendering (visible always true for active beat layer). */
export function beatInkAsScene(ink: BeatInk): {
  strokes: readonly DrawStroke[];
  visible: boolean;
} {
  return {
    strokes: ink.strokes,
    visible: true,
  };
}

export { EMPTY_SCENE };
