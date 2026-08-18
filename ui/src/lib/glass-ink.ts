/**
 * Glass ink — per-context annotations on the mirror.
 *
 * Extends the beat-ink law (lib/reel-annotate.ts: "ink is 1:1 with a beat —
 * not a global pen floating across the app") to every view: annotation is
 * deictic, so it is keyed by what it points at and painted only while that
 * context is on screen. The Draw tab's board (draw$) stays a separate,
 * global document. Never observed history — ui.* / localStorage only.
 */

import type { HashRoute } from "@/lib/hash-route";
import { normalizeStrokes, type DrawStroke } from "@/lib/draw";

/** Context identity for glass ink, or null when another surface owns ink
 *  (Draw tab → board scene; reels player → beat-ink store). */
export function routeGlassKey(route: HashRoute): string | null {
  if (route.view === "draw") return null;
  if (route.view === "reels" && route.reelId) return null;
  return route.sessionId ? `${route.view}:${route.sessionId}` : route.view;
}

export const GLASS_INK_STORAGE_KEY = "openstory.glass-ink.v1";
export const GLASS_INK_VERSION = 1;
/** Soft cap: keep the N most recently touched contexts. */
export const GLASS_INK_MAX_CONTEXTS = 40;

export type GlassInk = {
  readonly key: string;
  readonly strokes: readonly DrawStroke[];
  readonly updatedAt: string;
};

export type GlassInkStore = {
  readonly v: number;
  readonly byKey: Readonly<Record<string, GlassInk>>;
};

export function emptyGlassInkStore(): GlassInkStore {
  return { v: GLASS_INK_VERSION, byKey: {} };
}

export function getGlassInk(store: GlassInkStore, key: string): GlassInk {
  return store.byKey[key] ?? { key, strokes: [], updatedAt: "" };
}

export function setGlassInk(
  store: GlassInkStore,
  key: string,
  strokes: readonly DrawStroke[],
  opts?: { now?: () => string },
): GlassInkStore {
  const updatedAt = opts?.now?.() ?? new Date().toISOString();
  if (strokes.length === 0) {
    const { [key]: _drop, ...rest } = store.byKey;
    void _drop;
    return { v: GLASS_INK_VERSION, byKey: rest };
  }
  return {
    v: GLASS_INK_VERSION,
    byKey: { ...store.byKey, [key]: { key, strokes: [...strokes], updatedAt } },
  };
}

export function appendGlassInk(
  store: GlassInkStore,
  key: string,
  more: readonly DrawStroke[],
  opts?: { now?: () => string },
): GlassInkStore {
  if (more.length === 0) return store;
  const cur = getGlassInk(store, key);
  return setGlassInk(store, key, [...cur.strokes, ...more], opts);
}

export function clearGlassInk(store: GlassInkStore, key: string): GlassInkStore {
  return setGlassInk(store, key, []);
}

export function applyGlassInkIntent(
  store: GlassInkStore,
  intent: {
    readonly key: string;
    readonly clear?: boolean;
    readonly strokes?: readonly DrawStroke[];
    readonly mode?: "append" | "replace";
  },
  opts?: { now?: () => string },
): GlassInkStore {
  const strokes = intent.strokes ?? [];
  const replace = intent.clear === true || intent.mode === "replace";
  return replace
    ? setGlassInk(store, intent.key, strokes, opts)
    : appendGlassInk(store, intent.key, strokes, opts);
}

export function pruneGlassInkStore(store: GlassInkStore, max = GLASS_INK_MAX_CONTEXTS): GlassInkStore {
  const rows = Object.values(store.byKey);
  if (rows.length <= max) return store;
  const keep = [...rows]
    .sort((a, b) => (a.updatedAt < b.updatedAt ? 1 : -1))
    .slice(0, max);
  const byKey: Record<string, GlassInk> = {};
  for (const r of keep) byKey[r.key] = r;
  return { v: GLASS_INK_VERSION, byKey };
}

export function normalizeGlassInkStore(raw: unknown): GlassInkStore {
  if (!raw || typeof raw !== "object") return emptyGlassInkStore();
  const o = raw as Record<string, unknown>;
  if (o.v !== GLASS_INK_VERSION) return emptyGlassInkStore();
  const by = o.byKey;
  if (!by || typeof by !== "object") return emptyGlassInkStore();
  const out: Record<string, GlassInk> = {};
  for (const [k, v] of Object.entries(by as Record<string, unknown>)) {
    if (!k || !v || typeof v !== "object") continue;
    const row = v as Record<string, unknown>;
    const strokes = normalizeStrokes(row.strokes);
    if (strokes.length === 0) continue;
    out[k] = {
      key: k,
      strokes,
      updatedAt: typeof row.updatedAt === "string" ? row.updatedAt : "",
    };
  }
  return { v: GLASS_INK_VERSION, byKey: out };
}

export function loadGlassInkStore(
  storage: Pick<Storage, "getItem"> | null = typeof localStorage !== "undefined" ? localStorage : null,
): GlassInkStore {
  if (!storage) return emptyGlassInkStore();
  try {
    const text = storage.getItem(GLASS_INK_STORAGE_KEY);
    if (!text) return emptyGlassInkStore();
    return normalizeGlassInkStore(JSON.parse(text) as unknown);
  } catch {
    return emptyGlassInkStore();
  }
}

export function saveGlassInkStore(
  store: GlassInkStore,
  storage: Pick<Storage, "setItem" | "removeItem"> | null = typeof localStorage !== "undefined"
    ? localStorage
    : null,
): boolean {
  if (!storage) return false;
  try {
    if (Object.keys(store.byKey).length === 0) {
      storage.removeItem(GLASS_INK_STORAGE_KEY);
      return true;
    }
    storage.setItem(GLASS_INK_STORAGE_KEY, JSON.stringify(pruneGlassInkStore(store)));
    return true;
  } catch {
    return false;
  }
}
