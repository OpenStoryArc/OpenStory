//! Spec: buildSessionState$ — enriched pipeline integration.
//!
//! After feat/lazy-load-initial-state, the WS handshake is sidebar-only
//! (patterns + sessionLabels). Records arrive via `session_records_loaded`
//! actions dispatched from REST fetches, and via live `enriched` deltas.

import { describe, it, expect } from "vitest";
import { Subject, firstValueFrom, take, toArray } from "rxjs";

import type { WsMessage } from "@/types/websocket";
import type { WireRecord } from "@/types/wire-record";
import {
  buildSessionState$,
  dispatchSessionRecordsLoaded,
  type EnrichedSessionState,
} from "@/streams/sessions";

function makeWireRecord(overrides: Partial<WireRecord> = {}): WireRecord {
  return {
    id: "rec-1",
    seq: 1,
    session_id: "s1",
    timestamp: "2025-01-01T00:00:00Z",
    record_type: "user_message",
    payload: { content: "hello" },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 5,
    ...overrides,
  } as WireRecord;
}

describe("buildSessionState$ with enriched messages", () => {
  it("emits EMPTY_ENRICHED_STATE as initial value", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });
    const first = await firstValueFrom(state$);
    expect(first.records).toEqual([]);
    expect(first.currentEphemeral).toBeNull();
    expect(first.sessionLabels).toEqual({});
    expect(first.loadedSessions.size).toBe(0);
  });

  it("seeds sessionLabels from initial_state without populating records", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(2), toArray()));
    ws$.next({
      kind: "initial_state",
      session_labels: { s1: { label: "first prompt", branch: "main" } },
      patterns: [],
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    expect(final.sessionLabels.s1?.label).toBe("first prompt");
    expect(final.records).toHaveLength(0);
  });

  it("dispatchSessionRecordsLoaded merges REST records into the global stream", async () => {
    // Note: the external action subject is a module-level singleton, so
    // we disambiguate this test by using a unique session id and only
    // counting records belonging to it.
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(2), toArray()));
    dispatchSessionRecordsLoaded("build-test-session", [
      makeWireRecord({
        id: "build-test-r1",
        session_id: "build-test-session",
        depth: 0,
        parent_uuid: null,
      }),
      makeWireRecord({
        id: "build-test-r2",
        seq: 2,
        session_id: "build-test-session",
        depth: 1,
        parent_uuid: "build-test-r1",
      }),
    ]);
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    const own = final.records.filter((r) => r.session_id === "build-test-session");
    expect(own).toHaveLength(2);
    expect(own[1]!.depth).toBe(1);
    expect(own[1]!.parent_uuid).toBe("build-test-r1");
    expect(final.loadedSessions.has("build-test-session")).toBe(true);
  });

  it("appends enriched durable records preserving tree metadata", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const promise = firstValueFrom(state$.pipe(take(3), toArray()));
    ws$.next({
      kind: "initial_state",
      session_labels: {},
      patterns: [],
    });
    ws$.next({
      kind: "enriched",
      session_id: "enriched-test-session",
      records: [
        makeWireRecord({
          id: "enriched-test-r2",
          seq: 2,
          session_id: "enriched-test-session",
          depth: 3,
          parent_uuid: "enriched-test-r1",
        }),
      ],
      ephemeral: [],
      filter_deltas: {},
    });
    ws$.complete();

    const states = await promise;
    const final = states[states.length - 1]! as EnrichedSessionState;
    const r = final.records.find((x) => x.id === "enriched-test-r2");
    expect(r).toBeDefined();
    expect(r!.depth).toBe(3);
    expect(r!.parent_uuid).toBe("enriched-test-r1");
  });
});
