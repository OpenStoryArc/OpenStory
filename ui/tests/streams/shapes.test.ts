//! Spec: buildShapesStream$ / shapesReducer — live shape stream folding.
//!
//! The UI is a sink: the server sends `shapes` messages (new rows + running
//! counts) and `initial_state` (handshake counts); the reducer folds them into
//! a per-session counts map + a recent-rows feed. No analysis client-side.

import { describe, it, expect } from "vitest";
import { Subject, firstValueFrom, take, toArray } from "rxjs";
import { scenario } from "../bdd";
import type { WsMessage, ShapeCounts, ShapeRow } from "@/types/websocket";
import {
  buildShapesStream$,
  shapesReducer,
  EMPTY_SHAPES_STATE,
  RECENT_CAP,
} from "@/streams/shapes";

function counts(over: Partial<ShapeCounts> = {}): ShapeCounts {
  return {
    bash: 0, path: 0, change: 0, lines_added: 0, lines_removed: 0,
    programs: {}, top_segments: {}, ...over,
  };
}

function row(id: string, shape_type = "bash-shape"): ShapeRow {
  return {
    id, session_id: "s1", shape_type, seq: 0,
    timestamp: "2026-01-01T00:00:00Z", event_id: id, data: {},
  };
}

describe("shapesReducer", () => {
  it("seeds counts from the initial_state handshake", () => {
    scenario(
      () => ({ kind: "initial_state", shape_counts: { s1: counts({ bash: 3 }) } } as WsMessage),
      (msg) => shapesReducer(EMPTY_SHAPES_STATE, msg),
      (state) => expect(state.counts.s1?.bash).toBe(3),
    );
  });

  it("replaces a session's counts and prepends new rows on a shapes message", () => {
    scenario(
      () => ({
        kind: "shapes", session_id: "s1",
        shapes: [row("r2"), row("r1")],
        counts: counts({ bash: 2, programs: { git: 2 } }),
      } as WsMessage),
      (msg) => shapesReducer(EMPTY_SHAPES_STATE, msg),
      (state) => {
        expect(state.counts.s1?.bash).toBe(2);
        expect(state.counts.s1?.programs.git).toBe(2);
        expect(state.recent.map((r) => r.id)).toEqual(["r2", "r1"]);
      },
    );
  });

  it("caps the recent feed at RECENT_CAP, newest first", () => {
    scenario(
      () => Array.from({ length: RECENT_CAP + 10 }, (_, i) => row(`r${i}`)),
      (rows) => shapesReducer(EMPTY_SHAPES_STATE, {
        kind: "shapes", session_id: "s1", shapes: rows, counts: counts(),
      } as WsMessage),
      (state) => expect(state.recent.length).toBe(RECENT_CAP),
    );
  });
});

describe("buildShapesStream$", () => {
  it("folds a sequence of messages into accumulating per-session state", async () => {
    const ws$ = new Subject<WsMessage>();
    const out$ = buildShapesStream$(ws$);
    const promise = firstValueFrom(out$.pipe(take(3), toArray()));

    ws$.next({ kind: "shapes", session_id: "a", shapes: [row("a1")], counts: counts({ bash: 1 }) } as WsMessage);
    ws$.next({ kind: "shapes", session_id: "b", shapes: [row("b1")], counts: counts({ path: 1 }) } as WsMessage);
    ws$.complete();

    const emissions = await promise;
    const last = emissions[emissions.length - 1]!;
    expect(last.counts.a?.bash).toBe(1);
    expect(last.counts.b?.path).toBe(1);
    expect(last.recent.map((r) => r.id)).toEqual(["b1", "a1"]);
  });

  it("ignores unrelated message kinds", async () => {
    const ws$ = new Subject<WsMessage>();
    const out$ = buildShapesStream$(ws$);
    const promise = firstValueFrom(out$.pipe(take(2), toArray()));

    ws$.next({ kind: "plan_saved", session_id: "x" } as WsMessage);
    ws$.next({ kind: "shapes", session_id: "a", shapes: [], counts: counts({ change: 5 }) } as WsMessage);
    ws$.complete();

    const emissions = await promise;
    expect(emissions[emissions.length - 1]!.counts.a?.change).toBe(5);
  });
});
