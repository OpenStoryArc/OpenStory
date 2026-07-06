import { describe, it, expect } from "vitest";
import { inRange, sortProjectsByRecency } from "@/lib/explore";
import type { SessionSummary } from "@/types/session";

const NOW = Date.parse("2026-07-02T14:00:00Z");
const iso = (ms: number) => new Date(ms).toISOString();
const sess = (id: string, last: string, project?: string): SessionSummary =>
  ({ session_id: id, status: "completed", start_time: last, last_event: last, event_count: 1, project_name: project } as SessionSummary);

describe("inRange — filter a session's last_event to a date window", () => {
  it("all → always true", () => {
    expect(inRange(iso(NOW - 100 * 864e5), "all", NOW)).toBe(true);
  });
  it("7d → within 7 days true, older false", () => {
    expect(inRange(iso(NOW - 3 * 864e5), "7d", NOW)).toBe(true);
    expect(inRange(iso(NOW - 8 * 864e5), "7d", NOW)).toBe(false);
  });
  it("30d → within 30 days true, older false", () => {
    expect(inRange(iso(NOW - 20 * 864e5), "30d", NOW)).toBe(true);
    expect(inRange(iso(NOW - 40 * 864e5), "30d", NOW)).toBe(false);
  });
  it("today → now is in, five days ago is out", () => {
    expect(inRange(iso(NOW), "today", NOW)).toBe(true);
    expect(inRange(iso(NOW - 5 * 864e5), "today", NOW)).toBe(false);
  });
  it("custom {from,to} → inclusive window", () => {
    const range = { from: "2026-07-01", to: "2026-07-03" };
    expect(inRange("2026-07-02T09:00:00Z", range, NOW)).toBe(true);
    expect(inRange("2026-06-30T09:00:00Z", range, NOW)).toBe(false);
  });
  it("invalid timestamp → false (never crashes)", () => {
    expect(inRange("nope", "7d", NOW)).toBe(false);
  });
});

describe("sortProjectsByRecency — projects ordered by their most-recent session", () => {
  it("orders groups by newest last_event, sessions within a group newest-first", () => {
    const sessions = [
      sess("a1", iso(NOW - 10 * 864e5), "alpha"),
      sess("b1", iso(NOW - 1 * 864e5), "beta"),
      sess("a2", iso(NOW - 2 * 864e5), "alpha"),
    ];
    const groups = sortProjectsByRecency(sessions);
    expect(groups.map((g) => g.project)).toEqual(["beta", "alpha"]); // beta touched more recently
    const alpha = groups.find((g) => g.project === "alpha")!;
    expect(alpha.sessions.map((s) => s.session_id)).toEqual(["a2", "a1"]); // newest first within group
    expect(alpha.latest).toBe(iso(NOW - 2 * 864e5));
  });
});
