/** Live shape stream — folds `shapes` + `initial_state` WS messages into a
 *  per-session counts map plus a recent-rows feed. Pure reducer; no analysis
 *  here (the server already did it) — the UI is a sink. */

import { Observable } from "rxjs";
import { scan, filter, startWith, shareReplay } from "rxjs/operators";
import type { WsMessage, ShapeCounts, ShapeRow } from "@/types/websocket";

/** How many recent rows to keep in the live feed. */
export const RECENT_CAP = 50;

export interface ShapesState {
  /** session_id → running counts (latest from the server). */
  readonly counts: Readonly<Record<string, ShapeCounts>>;
  /** Newest-first feed of recently-streamed shape rows (capped). */
  readonly recent: readonly ShapeRow[];
}

export const EMPTY_SHAPES_STATE: ShapesState = { counts: {}, recent: [] };

/** Fold one WS message into shapes state. Pure. */
export function shapesReducer(state: ShapesState, msg: WsMessage): ShapesState {
  if (msg.kind === "initial_state") {
    // Seed counts from the handshake; don't clobber anything already live.
    return { ...state, counts: { ...(msg.shape_counts ?? {}), ...state.counts } };
  }
  if (msg.kind === "shapes") {
    return {
      counts: { ...state.counts, [msg.session_id]: msg.counts },
      recent: [...msg.shapes, ...state.recent].slice(0, RECENT_CAP),
    };
  }
  return state;
}

/** Build the live shapes state observable from the raw WS message stream. */
export function buildShapesStream$(ws$: Observable<WsMessage>): Observable<ShapesState> {
  return ws$.pipe(
    filter((m) => m.kind === "shapes" || m.kind === "initial_state"),
    scan(shapesReducer, EMPTY_SHAPES_STATE),
    startWith(EMPTY_SHAPES_STATE),
    shareReplay(1),
  );
}
