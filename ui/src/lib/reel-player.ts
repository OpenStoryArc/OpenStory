/** Reel player — pure stop-sequencing state machine. No React, no I/O, no
 *  TTS: the component layer maps utterance-end / timer / click / Space to
 *  ADVANCE and Esc to EXIT. Deterministic by construction. */

export type ReelPlayerState =
  | { readonly phase: "idle" }
  | { readonly phase: "opener" }
  | { readonly phase: "stop"; readonly index: number }
  | { readonly phase: "closer" }
  | { readonly phase: "done" };

export type ReelPlayerEvent =
  | { type: "PLAY" }
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

export function reelPlayerReduce(
  state: ReelPlayerState,
  event: ReelPlayerEvent,
  ctx: ReelPlayerCtx,
): ReelPlayerState {
  if (event.type === "EXIT") return { phase: "idle" };
  if (event.type === "PLAY") {
    if (ctx.stopCount === 0) return { phase: "idle" };
    return ctx.hasOpener ? { phase: "opener" } : { phase: "stop", index: 0 };
  }
  if (event.type === "JUMP") {
    if (event.index >= 0 && event.index < ctx.stopCount) {
      return { phase: "stop", index: event.index };
    }
    return state;
  }
  if (event.type === "BACK") {
    switch (state.phase) {
      case "stop":
        if (state.index > 0) return { phase: "stop", index: state.index - 1 };
        return ctx.hasOpener ? { phase: "opener" } : state;
      case "closer":
        return ctx.stopCount > 0 ? { phase: "stop", index: ctx.stopCount - 1 } : state;
      default:
        return state;
    }
  }
  // ADVANCE
  switch (state.phase) {
    case "opener":
      return { phase: "stop", index: 0 };
    case "stop": {
      const next = state.index + 1;
      if (next < ctx.stopCount) return { phase: "stop", index: next };
      return ctx.hasCloser ? { phase: "closer" } : { phase: "done" };
    }
    case "closer":
      return { phase: "done" };
    default:
      return state;
  }
}
