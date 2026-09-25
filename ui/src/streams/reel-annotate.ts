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
  hydrateBeatInkFromReel,
  loadBeatInkStore,
  saveBeatInkStore,
  type BeatInk,
  type BeatInkStore,
  type BeatKey,
} from "@/lib/reel-annotate";
import { putReelBeatInk, type Reel } from "@/lib/reels-api";
import { normalizeStrokes } from "@/lib/draw";

const store$ = new BehaviorSubject<BeatInkStore>(loadBeatInkStore());
const activeKey$ = new BehaviorSubject<BeatKey | null>(null);

/** Local only: the in-memory subject plus the localStorage cache. */
function persist(next: BeatInkStore): void {
  store$.next(next);
  saveBeatInkStore(next);
}

/**
 * Local + write-through: after a change to `key`, carry that beat's full
 * stroke list to the reel record. Fire-and-forget — if the server is
 * unreachable the local copy stays, and the next fetch of the reel will
 * reconcile (server wins).
 */
function persistAndPush(next: BeatInkStore, key: BeatKey): void {
  persist(next);
  const ink = getBeatInk(next, key);
  void putReelBeatInk(key.reelId, key.beatIndex, ink.strokes, ink.updatedAt);
}

/**
 * A reel just arrived from the server.
 * - Server has ink for it → that copy is truth; stale local beats go.
 * - Server has none but this browser does → migrate the local beats up
 *   (ink drawn before the reel record could hold it is not thrown away).
 */
export function hydrateFromReel(reel: Reel): void {
  const serverHasInk = Object.keys(reel.beatInk ?? {}).length > 0;
  if (!serverHasInk) {
    const local = Object.values(store$.value.byKey).filter((ink) => ink.reelId === reel.id);
    if (local.length > 0) {
      for (const ink of local) {
        void putReelBeatInk(ink.reelId, ink.beatIndex, ink.strokes, ink.updatedAt);
      }
      return;
    }
  }
  persist(hydrateBeatInkFromReel(store$.value, reel));
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
  persistAndPush(next, key);
  return getBeatInk(next, key);
}

export function clearActiveBeatInk(): void {
  const key = activeKey$.value;
  if (!key) return;
  persistAndPush(clearBeatInk(store$.value, key), key);
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
  const key: BeatKey = { reelId: intent.reelId, beatIndex: intent.beatIndex };
  persistAndPush(next, key);
  return getBeatInk(next, key);
}
