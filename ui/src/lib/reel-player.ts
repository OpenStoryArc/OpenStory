/** Reel player — pure stop-sequencing state machine. No React, no I/O, no
 *  TTS: the component layer maps utterance-end / timer / click / Space to
 *  ADVANCE and Esc to EXIT. Deterministic by construction.
 *
 *  `paused` freezes autoplay (TTS/timer must not ADVANCE while paused).
 *  Manual ADVANCE/BACK/JUMP still work so you can click through slides. */

export type ReelPlayerState =
  | { readonly phase: "idle" }
  | { readonly phase: "opener"; readonly paused: boolean }
  | { readonly phase: "stop"; readonly index: number; readonly paused: boolean }
  | { readonly phase: "closer"; readonly paused: boolean }
  | { readonly phase: "done" };

export type ReelPlayerEvent =
  | { type: "PLAY" }
  | { type: "PAUSE" }
  | { type: "RESUME" }
  | { type: "TOGGLE_PAUSE" }
  | { type: "ADVANCE" }
  | { type: "BACK" }
  | { type: "JUMP"; index: number }
  | { type: "EXIT" };

export interface ReelPlayerCtx {
  readonly stopCount: number;
  readonly hasCloser: boolean;
  /** BLUF title card before stop 0 — optional so pre-opener callers/tests
   *  keep their two-field ctx shape unchanged. */
  readonly hasOpener?: boolean;
}

export const initialReelPlayerState: ReelPlayerState = { phase: "idle" };

function withPaused(
  state: ReelPlayerState,
  paused: boolean,
): ReelPlayerState {
  if (state.phase === "opener" || state.phase === "stop" || state.phase === "closer") {
    return { ...state, paused };
  }
  return state;
}

export function isReelPlaying(state: ReelPlayerState): boolean {
  return (
    (state.phase === "opener" || state.phase === "stop" || state.phase === "closer") &&
    !state.paused
  );
}

export function isReelPaused(state: ReelPlayerState): boolean {
  return (
    (state.phase === "opener" || state.phase === "stop" || state.phase === "closer") &&
    state.paused
  );
}

export function reelPlayerReduce(
  state: ReelPlayerState,
  event: ReelPlayerEvent,
  ctx: ReelPlayerCtx,
): ReelPlayerState {
  if (event.type === "EXIT") return { phase: "idle" };
  if (event.type === "PAUSE") return withPaused(state, true);
  if (event.type === "RESUME") return withPaused(state, false);
  if (event.type === "TOGGLE_PAUSE") {
    if (state.phase === "opener" || state.phase === "stop" || state.phase === "closer") {
      return withPaused(state, !state.paused);
    }
    return state;
  }
  if (event.type === "PLAY") {
    if (ctx.stopCount === 0) return { phase: "idle" };
    // Start unpaused (autoplay on).
    return ctx.hasOpener
      ? { phase: "opener", paused: false }
      : { phase: "stop", index: 0, paused: false };
  }
  // Manual navigation preserves pause so click-through while paused stays paused.
  const paused =
    state.phase === "opener" || state.phase === "stop" || state.phase === "closer"
      ? state.paused
      : false;
  if (event.type === "JUMP") {
    if (event.index >= 0 && event.index < ctx.stopCount) {
      return { phase: "stop", index: event.index, paused };
    }
    return state;
  }
  if (event.type === "BACK") {
    switch (state.phase) {
      case "stop":
        if (state.index > 0) return { phase: "stop", index: state.index - 1, paused };
        return ctx.hasOpener ? { phase: "opener", paused } : state;
      case "closer":
        return ctx.stopCount > 0
          ? { phase: "stop", index: ctx.stopCount - 1, paused }
          : state;
      default:
        return state;
    }
  }
  // ADVANCE
  switch (state.phase) {
    case "opener":
      return { phase: "stop", index: 0, paused };
    case "stop": {
      const next = state.index + 1;
      if (next < ctx.stopCount) return { phase: "stop", index: next, paused };
      return ctx.hasCloser ? { phase: "closer", paused } : { phase: "done" };
    }
    case "closer":
      return { phase: "done" };
    default:
      return state;
  }
}
