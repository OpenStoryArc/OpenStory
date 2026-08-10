/**
 * draw$ — agent pen / attention canvas scene stream (ui.* only).
 * Overlay and Draw tab are pure sinks of this subject.
 * Hydrates from localStorage; debounced pen-eyes reporter for agents.
 */

import { BehaviorSubject, type Observable } from "rxjs";
import {
  applyDrawIntent,
  EMPTY_SCENE,
  normalizeStrokes,
  type DrawScene,
  type DrawStroke,
} from "@/lib/draw";
import { loadPersistedScene, savePersistedScene } from "@/lib/draw-persist";

const initial = loadPersistedScene();
const scene$ = new BehaviorSubject<DrawScene>(initial);

/**
 * When true, DrawOverlay captures pointer for freehand (annotate on reels /
 * story / explore). When false, overlay is pointer-events none so the mirror
 * stays clickable. Not persisted — session UI preference.
 */
const interactive$ = new BehaviorSubject<boolean>(false);

/** Side-effect hook: App installs debounced pen-eyes report here. */
let penSceneReporter: ((scene: DrawScene) => void) | null = null;

export function setPenSceneReporter(
  fn: ((scene: DrawScene) => void) | null,
): void {
  penSceneReporter = fn;
}

function notifyPenEyes(scene: DrawScene): void {
  try {
    penSceneReporter?.(scene);
  } catch {
    // never break ink for telemetry
  }
}

function persist(scene: DrawScene): void {
  try {
    savePersistedScene(scene);
  } catch {
    // quota / private mode — keep in-memory scene
  }
}

function publish(next: DrawScene): DrawScene {
  scene$.next(next);
  persist(next);
  notifyPenEyes(next);
  return next;
}

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
      ? normalizeStrokes(intent.strokes)
      : normalizeStrokes(intent.strokes)
    : undefined;
  const next = applyDrawIntent(scene$.value, {
    clear: intent.clear,
    strokes,
    visible: intent.visible,
    label: intent.label,
    mode: intent.mode,
  });
  return publish(next);
}

export function clearDraw(): void {
  publish({ ...EMPTY_SCENE, visible: true });
}

/** Hide/show ink without clearing strokes (navigate with board retained). */
export function setDrawVisible(visible: boolean): DrawScene {
  const cur = scene$.value;
  if (cur.visible === visible) return cur;
  return publish({ ...cur, visible });
}

export function toggleDrawVisible(): DrawScene {
  return setDrawVisible(!scene$.value.visible);
}

export function drawInteractive$(): Observable<boolean> {
  return interactive$.asObservable();
}

export function getDrawInteractive(): boolean {
  return interactive$.value;
}

/** Annotate mode: freehand on the glass over history / reels. */
export function setDrawInteractive(on: boolean): void {
  interactive$.next(on === true);
  // Annotating implies ink should be visible.
  if (on && !scene$.value.visible) {
    setDrawVisible(true);
  }
}

export function toggleDrawInteractive(): boolean {
  const next = !interactive$.value;
  setDrawInteractive(next);
  return next;
}
