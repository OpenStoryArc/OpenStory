import { describe, it, expect } from "vitest";
import { Subject, firstValueFrom, take, toArray } from "rxjs";
import { buildSessionState$, type EnrichedSessionState } from "@/streams/sessions";
import type { WsMessage } from "@/types/websocket";
import type { ViewRecord } from "@/types/view-record";

let seqCounter = 0;

function makeViewRecord(overrides: Partial<ViewRecord> = {}): ViewRecord {
  seqCounter++;
  return {
    id: overrides.id ?? `rec-${seqCounter}`,
    seq: overrides.seq ?? seqCounter,
    session_id: overrides.session_id ?? "s1",
    timestamp: overrides.timestamp ?? "2026-01-01T00:00:00Z",
    record_type: overrides.record_type ?? "user_message",
    agent_id: overrides.agent_id ?? null,
    is_sidechain: overrides.is_sidechain ?? false,
    payload: overrides.payload ?? { content: "test prompt" },
  };
}

describe("buildSessionState$", () => {
  it("emits initial empty state", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$);
    const state = await firstValueFrom(state$);
    expect(state.records).toHaveLength(0);
    ws$.complete();
  });

  it("processes new sidebar-only initial_state (no records seeded)", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$);

    const statePromise = firstValueFrom(state$.pipe(take(2), toArray()));

    ws$.next({
      kind: "initial_state",
      session_labels: { s1: { label: "first prompt", branch: null } },
      patterns: [],
    });
    ws$.complete();

    const states = await statePromise;
    const last = states[states.length - 1]! as EnrichedSessionState;
    expect(last.records).toHaveLength(0);
    expect(last.sessionLabels.s1?.label).toBe("first prompt");
  });

  it("processes legacy view_records message — appends", async () => {
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$);

    const statePromise = firstValueFrom(state$.pipe(take(2), toArray()));

    ws$.next({
      kind: "view_records",
      session_id: "s1",
      view_records: [makeViewRecord()],
    });
    ws$.complete();

    const states = await statePromise;
    const last = states[states.length - 1]! as EnrichedSessionState;
    expect(last.records).toHaveLength(1);
  });

  it("accumulates state across multiple messages", async () => {
    // batchMs=0 disables the bufferTime window so each message
    // produces its own emission (otherwise the two synchronous nexts
    // collapse into a single batched emission and take(3) hangs).
    const ws$ = new Subject<WsMessage>();
    const state$ = buildSessionState$(ws$, { batchMs: 0 });

    const statePromise = firstValueFrom(state$.pipe(take(3), toArray()));

    ws$.next({
      kind: "view_records",
      session_id: "s1",
      view_records: [makeViewRecord({ id: "r1" })],
    });
    ws$.next({
      kind: "view_records",
      session_id: "s1",
      view_records: [makeViewRecord({ id: "r2" })],
    });
    ws$.complete();

    const states = await statePromise;
    const last = states[states.length - 1]! as EnrichedSessionState;
    expect(last.records).toHaveLength(2);
  });
});
