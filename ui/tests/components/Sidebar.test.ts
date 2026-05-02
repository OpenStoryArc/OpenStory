//! Spec: deriveSessions — pure data derivation from event arrays.
//!
//! Boundary table: empty, single session, multiple sessions, subagents,
//! agent_id presence/absence, ordering by latest timestamp.

import { describe, it, expect } from "vitest";
import { deriveSessions } from "@/components/Sidebar";
import type { WireRecord } from "@/types/wire-record";
import type { SessionLabel } from "@/types/websocket";

/** Minimal WireRecord factory for sidebar tests. */
function makeEvent(overrides: Partial<WireRecord> & { session_id: string; timestamp: string }): WireRecord {
  return {
    id: `evt-${Math.random().toString(36).slice(2, 8)}`,
    seq: 1,
    record_type: "tool_call",
    payload: { name: "Read", call_id: "c1", input: {} },
    agent_id: null,
    is_sidechain: false,
    depth: 0,
    parent_uuid: null,
    truncated: false,
    payload_bytes: 0,
    ...overrides,
  } as WireRecord;
}

describe("deriveSessions", () => {
  it("should return empty array for no events", () => {
    expect(deriveSessions([])).toEqual([]);
  });

  it("should group events by session_id", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:02Z" }),
      makeEvent({ session_id: "s2", timestamp: "2026-01-01T00:00:03Z" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions).toHaveLength(2);
    const s1 = sessions.find((s) => s.id === "s1")!;
    const s2 = sessions.find((s) => s.id === "s2")!;
    expect(s1.eventCount).toBe(2);
    expect(s2.eventCount).toBe(1);
  });

  it("should track latest timestamp per session", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:05Z" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:03Z" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.latestTimestamp).toBe("2026-01-01T00:00:05Z");
  });

  it("should sort sessions by latest timestamp descending (most recent first)", () => {
    const events = [
      makeEvent({ session_id: "old", timestamp: "2026-01-01T00:00:01Z" }),
      makeEvent({ session_id: "new", timestamp: "2026-01-01T00:00:10Z" }),
      makeEvent({ session_id: "mid", timestamp: "2026-01-01T00:00:05Z" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions.map((s) => s.id)).toEqual(["new", "mid", "old"]);
  });

  it("should count main agent events (agent_id === null)", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z", agent_id: null }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:02Z", agent_id: null }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:03Z", agent_id: "sub-1" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.mainAgentCount).toBe(2);
  });

  it("should identify subagents with event counts and first timestamp", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z", agent_id: null }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:05Z", agent_id: "sub-a" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:03Z", agent_id: "sub-a" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:10Z", agent_id: "sub-b" }),
    ];
    const sessions = deriveSessions(events);
    const s = sessions[0]!;
    expect(s.subagents).toHaveLength(2);
    // sub-a first timestamp should be the earlier one
    const subA = s.subagents.find((sa) => sa.agentId === "sub-a")!;
    expect(subA.eventCount).toBe(2);
    expect(subA.firstTimestamp).toBe("2026-01-01T00:00:03Z");
    const subB = s.subagents.find((sa) => sa.agentId === "sub-b")!;
    expect(subB.eventCount).toBe(1);
  });

  it("should sort subagents by first timestamp ascending", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:10Z", agent_id: "late-agent" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z", agent_id: "early-agent" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:05Z", agent_id: "mid-agent" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.subagents.map((s) => s.agentId)).toEqual([
      "early-agent",
      "mid-agent",
      "late-agent",
    ]);
  });

  it("should return empty subagents array when no agent_id events exist", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z", agent_id: null }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:02Z", agent_id: null }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.subagents).toEqual([]);
    expect(sessions[0]!.mainAgentCount).toBe(2);
  });

  it("should handle session with only subagent events (no main agent)", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z", agent_id: "sub-1" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:02Z", agent_id: "sub-1" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.mainAgentCount).toBe(0);
    expect(sessions[0]!.subagents).toHaveLength(1);
    expect(sessions[0]!.eventCount).toBe(2);
  });

  it("should return null label/branch when no labels provided", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.label).toBeNull();
    expect(sessions[0]!.branch).toBeNull();
  });

  it("should populate label and branch from sessionLabels map", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z" }),
    ];
    const sessionLabels: Record<string, SessionLabel> = {
      s1: { label: "Fix the login bug", branch: "feature/login-fix" },
    };
    const sessions = deriveSessions(events, sessionLabels);
    expect(sessions[0]!.label).toBe("Fix the login bug");
    expect(sessions[0]!.branch).toBe("feature/login-fix");
  });

  it("should leave subagent description null after the agent_labels feature was cut", () => {
    // The legacy `agent_labels` feature was removed in chore/cut-legacy-detectors
    // (broken end-to-end on real data). Subagent labels now fall through to
    // sessionLabels[sub.id]. See BACKLOG: "Subagent Task Labels — Restore After Cut"
    // for the planned proper fix.
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z", agent_id: "sub-a" }),
    ];
    const sessions = deriveSessions(events, undefined);
    expect(sessions[0]!.subagents[0]!.description).toBeNull();
  });

  // --- Plan count ---

  it("should count ExitPlanMode tool_calls as plans", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z" }),
      makeEvent({
        session_id: "s1",
        timestamp: "2026-01-01T00:00:02Z",
        record_type: "tool_call",
        payload: { name: "ExitPlanMode", call_id: "c1", input: {}, raw_input: {}, is_error: false },
      }),
      makeEvent({
        session_id: "s1",
        timestamp: "2026-01-01T00:00:03Z",
        record_type: "tool_call",
        payload: { name: "ExitPlanMode", call_id: "c2", input: {}, raw_input: {}, is_error: false },
      }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.planCount).toBe(2);
  });

  it("should return planCount 0 when no plan events exist", () => {
    const events = [
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:01Z" }),
      makeEvent({ session_id: "s1", timestamp: "2026-01-01T00:00:02Z" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.planCount).toBe(0);
  });

  it("should not count EnterPlanMode as a plan (only ExitPlanMode)", () => {
    const events = [
      makeEvent({
        session_id: "s1",
        timestamp: "2026-01-01T00:00:01Z",
        record_type: "tool_call",
        payload: { name: "EnterPlanMode", call_id: "c1", input: {}, raw_input: {}, is_error: false },
      }),
      makeEvent({
        session_id: "s1",
        timestamp: "2026-01-01T00:00:02Z",
        record_type: "tool_call",
        payload: { name: "ExitPlanMode", call_id: "c2", input: {}, raw_input: {}, is_error: false },
      }),
      makeEvent({
        session_id: "s1",
        timestamp: "2026-01-01T00:00:03Z",
        record_type: "tool_call",
        payload: { name: "EnterPlanMode", call_id: "c3", input: {}, raw_input: {}, is_error: false },
      }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.planCount).toBe(1);
  });

  it("should populate host and user from REST sessions", () => {
    const sessions = deriveSessions(
      [],
      undefined,
      [
        {
          session_id: "s-stamped",
          last_event: "2026-05-01T00:00:00Z",
          start_time: "2026-05-01T00:00:00Z",
          host: "Katies-Mac-mini",
          user: "katie",
        },
        {
          session_id: "s-legacy",
          last_event: "2026-05-01T00:00:01Z",
          start_time: "2026-05-01T00:00:01Z",
          // host + user omitted: legacy session pre-stamping
        },
      ],
    );

    const stamped = sessions.find((s) => s.id === "s-stamped")!;
    expect(stamped.host).toBe("Katies-Mac-mini");
    expect(stamped.user).toBe("katie");

    const legacy = sessions.find((s) => s.id === "s-legacy")!;
    expect(legacy.host).toBeNull();
    expect(legacy.user).toBeNull();
  });

  it("should default host and user to null for sessions derived from events alone", () => {
    // No restSessions arg → no origin info available → both null.
    const events = [
      makeEvent({ session_id: "s-no-rest", timestamp: "2026-05-01T00:00:00Z" }),
    ];
    const sessions = deriveSessions(events);
    expect(sessions[0]!.host).toBeNull();
    expect(sessions[0]!.user).toBeNull();
  });
});
