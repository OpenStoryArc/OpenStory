/**
 * draw$ — agent pen scene stream (ui.* only).
 * Overlay and Draw tab are pure sinks of this subject.
 */

import { BehaviorSubject, type Observable } from "rxjs";
import {
  applyDrawIntent,
  EMPTY_SCENE,
  normalizeStrokes,
  type DrawScene,
  type DrawStroke,
} from "@/lib/draw";

const scene$ = new BehaviorSubject<DrawScene>(EMPTY_SCENE);

export function drawScene$(): Observable<DrawScene> {
  return scene$.asObservable();
}

export function getDrawScene(): DrawScene {
  return scene$.value;
}

export function commitDraw(intent: {
  clear?: boolean;
  strokes?: readonly DrawStroke[] | unknown;
  visible?: boolean;
  label?: string;
  mode?: "append" | "replace";
}): DrawScene {
  const strokes = intent.strokes
    ? Array.isArray(intent.strokes) &&
      intent.strokes.length > 0 &&
      typeof intent.strokes[0] === "object" &&
      intent.strokes[0] !== null &&
      "type" in (intent.strokes[0] as object)
      ? // already DrawStroke-ish or wire
        normalizeStrokes(intent.strokes)
      : normalizeStrokes(intent.strokes)
    : undefined;
  const next = applyDrawIntent(scene$.value, {
    clear: intent.clear,
    strokes,
    visible: intent.visible,
    label: intent.label,
    mode: intent.mode,
  });
  scene$.next(next);
  return next;
}

export function clearDraw(): void {
  scene$.next({ ...EMPTY_SCENE, visible: true });
}
