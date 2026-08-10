/**
 * Beat-ink stream — marginalia keyed by reelId + beatIndex.
 */

import { BehaviorSubject, type Observable } from "rxjs";
import type { DrawStroke } from "@/lib/draw";
import {
  appendBeatInk,
  applyBeatInkIntent,
  clearBeatInk,
  emptyBeatInkStore,
  getBeatInk,
  loadBeatInkStore,
  saveBeatInkStore,
  type BeatInk,
  type BeatInkStore,
  type BeatKey,
} from "@/lib/reel-annotate";
import { normalizeStrokes } from "@/lib/draw";

const store$ = new BehaviorSubject<BeatInkStore>(loadBeatInkStore());
const activeKey$ = new BehaviorSubject<BeatKey | null>(null);

function persist(next: BeatInkStore): void {
  store$.next(next);
  saveBeatInkStore(next);
}

export function beatInkStore$(): Observable<BeatInkStore> {
  return store$.asObservable();
}

export function getBeatInkStore(): BeatInkStore {
  return store$.value;
}

export function activeBeatKey$(): Observable<BeatKey | null> {
  return activeKey$.asObservable();
}

export function getActiveBeatKey(): BeatKey | null {
  return activeKey$.value;
}

/** Player sets this when showing a stop; null when not on a beat stage. */
export function setActiveBeatKey(key: BeatKey | null): void {
  activeKey$.next(key);
}

export function getActiveBeatInk(): BeatInk | null {
  const key = activeKey$.value;
  if (!key) return null;
  return getBeatInk(store$.value, key);
}

export function appendActiveBeatStrokes(strokes: readonly DrawStroke[]): BeatInk | null {
  const key = activeKey$.value;
  if (!key || strokes.length === 0) return null;
  const next = appendBeatInk(store$.value, key, strokes);
  persist(next);
  return getBeatInk(next, key);
}

export function clearActiveBeatInk(): void {
  const key = activeKey$.value;
  if (!key) return;
  persist(clearBeatInk(store$.value, key));
}

export function clearAllBeatInk(): void {
  persist(emptyBeatInkStore());
}

/**
 * Agent parity: write ink to a specific slide without needing the player focus.
 * Returns the beat ink after apply.
 */
export function commitBeatInkIntent(intent: {
  reelId: string;
  beatIndex: number;
  clear?: boolean;
  strokes?: readonly unknown[];
  mode?: "append" | "replace";
}): BeatInk {
  const strokes = intent.strokes ? normalizeStrokes(intent.strokes) : [];
  const next = applyBeatInkIntent(store$.value, {
    reelId: intent.reelId,
    beatIndex: intent.beatIndex,
    clear: intent.clear,
    strokes,
    mode: intent.mode,
  });
  persist(next);
  return getBeatInk(next, { reelId: intent.reelId, beatIndex: intent.beatIndex });
}
