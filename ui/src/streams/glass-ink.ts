/**
 * Glass-ink stream — per-context annotations, keyed by routeGlassKey.
 */

import { BehaviorSubject, type Observable } from "rxjs";
import { normalizeStrokes, type DrawStroke } from "@/lib/draw";
import {
  appendGlassInk,
  applyGlassInkIntent,
  clearGlassInk,
  getGlassInk,
  loadGlassInkStore,
  saveGlassInkStore,
  type GlassInk,
  type GlassInkStore,
} from "@/lib/glass-ink";

const store$ = new BehaviorSubject<GlassInkStore>(loadGlassInkStore());

function persist(next: GlassInkStore): void {
  store$.next(next);
  saveGlassInkStore(next);
}

export function glassInkStore$(): Observable<GlassInkStore> {
  return store$.asObservable();
}

export function getGlassInkFor(key: string): GlassInk {
  return getGlassInk(store$.value, key);
}

export function appendGlassStrokes(key: string, strokes: readonly DrawStroke[]): GlassInk {
  persist(appendGlassInk(store$.value, key, strokes));
  return getGlassInk(store$.value, key);
}

export function clearGlassContext(key: string): void {
  persist(clearGlassInk(store$.value, key));
}

export function commitGlassInkIntent(intent: {
  key: string;
  clear?: boolean;
  strokes?: readonly unknown[];
  mode?: "append" | "replace";
}): GlassInk {
  const strokes = intent.strokes ? normalizeStrokes(intent.strokes) : [];
  persist(
    applyGlassInkIntent(store$.value, {
      key: intent.key,
      clear: intent.clear,
      strokes,
      mode: intent.mode,
    }),
  );
  return getGlassInk(store$.value, intent.key);
}
