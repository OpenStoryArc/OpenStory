/** fleet-presence — the Fleet panel's rows, from GET /api/fleet/presence
 *  (P-05). Pure: nodes in, rows with a level, an age label, and a self
 *  flag out. A stale node is critical whatever its last body said. */

import { describe, expect, it } from "vitest";
import { presenceRows, type PresenceNode } from "@/lib/fleet-presence";

const healthyBody = {
  status: "ok",
  boot: { phase: "serving" },
  bus: { connected: true },
  leaf: { configured: false, connected: false, hub: null },
  streams: [],
  consumers: {},
  watchers_detail: [],
};

const fresh: PresenceNode = {
  host: "node-a",
  principal_id: "dev",
  person_id: "person-1",
  time: "2026-09-24T00:00:00.000Z",
  age_secs: 5,
  stale: false,
  status: "ok",
  git_sha: "aaa111",
  version: "0.9.0",
  body: healthyBody,
};

const quiet: PresenceNode = {
  ...fresh,
  host: "node-b",
  principal_id: "hub",
  time: "2026-09-23T23:50:00.000Z",
  age_secs: 600,
  stale: true,
  git_sha: "bbb222",
};

const busDown: PresenceNode = {
  ...fresh,
  host: "node-c",
  person_id: null,
  age_secs: 8,
  git_sha: "ccc333",
  body: { ...healthyBody, bus: { connected: false } },
};

describe("when nodes report", () => {
  it("should give each node a level, an age label, and a self flag", () => {
    const [a, b, c] = presenceRows([fresh, quiet, busDown], "node-a");
    expect(a).toMatchObject({ host: "node-a", level: "ok", ageLabel: "5 s ago", isSelf: true, stale: false, gitSha: "aaa111" });
    expect(a?.findings).toEqual([]);
    expect(b).toMatchObject({ host: "node-b", level: "critical", ageLabel: "10 min ago", isSelf: false, stale: true });
    expect(b?.findings).toEqual(["no beat for 10 min (stale past 3 intervals)"]);
    expect(c).toMatchObject({ host: "node-c", level: "critical", isSelf: false, stale: false });
    expect(c?.findings).toEqual(["bus is not connected"]);
  });
});

describe("when a node has no readable time", () => {
  it("should say never and be critical", () => {
    const [row] = presenceRows([{ ...fresh, age_secs: null, stale: true }], null);
    expect(row?.ageLabel).toBe("never");
    expect(row?.level).toBe("critical");
  });
});

describe("when ages are long", () => {
  it("should label hours and days", () => {
    const [hours, days] = presenceRows(
      [
        { ...quiet, host: "h", age_secs: 7_200 },
        { ...quiet, host: "d", age_secs: 3 * 86_400 + 100 },
      ],
      null,
    );
    expect(hours?.ageLabel).toBe("2 h ago");
    expect(days?.ageLabel).toBe("3 d ago");
  });
});
