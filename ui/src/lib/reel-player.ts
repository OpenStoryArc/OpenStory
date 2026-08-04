/** Reel player — pure stop-sequencing state machine. No React, no I/O, no
 *  TTS: the component layer maps utterance-end / timer / click / Space to
 *  ADVANCE and Esc to EXIT. Deterministic by construction. */

export type ReelPlayerState =
  | { readonly phase: "idle" }
  | { readonly phase: "stop"; readonly index: number }
  | { readonly phase: "closer" }
  | { readonly phase: "done" };

export type ReelPlayerEvent = { type: "PLAY" } | { type: "ADVANCE" } | { type: "EXIT" };

export interface ReelPlayerCtx {
  readonly stopCount: number;
  readonly hasCloser: boolean;
}

export const initialReelPlayerState: ReelPlayerState = { phase: "idle" };

export function reelPlayerReduce(
  state: ReelPlayerState,
  event: ReelPlayerEvent,
  ctx: ReelPlayerCtx,
): ReelPlayerState {
  if (event.type === "EXIT") return { phase: "idle" };
  if (event.type === "PLAY") {
    return ctx.stopCount > 0 ? { phase: "stop", index: 0 } : { phase: "idle" };
  }
  // ADVANCE
  switch (state.phase) {
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
