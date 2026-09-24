/** fleet-presence — the Fleet panel's rows, from GET /api/fleet/presence
 *  (P-05). Pure: nodes in, rows with a level, an age label, and a self
 *  flag out. A stale node is critical whatever its last body said. */

import { describe, expect, it } from "vitest";
import { presenceRows } from "@/lib/fleet-presence";

const healthyBody = {
  status: "ok",
  boot: { phase: "serving" },
  bus: { connected: true },
  leaf: { configured: false, connected: false, hub: null },
  streams: [],
  consumers: {},
  watchers_detail: [],
};

const nodes = [
  {
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
  },
  {
    host: "node-b",
    principal_id: "hub",
    person_id: "person-1",
    time: "2026-09-23T23:50:00.000Z",
    age_secs: 600,
    stale: true,
    status: "ok",
    git_sha: "bbb222",
    version: "0.9.0",
    body: healthyBody,
  },
  {
    host: "node-c",
    principal_id: "dev",
    person_id: null,
    time: "2026-09-24T00:00:00.000Z",
    age_secs: 8,
    stale: false,
    status: "ok",
    git_sha: "ccc333",
    version: "0.9.0",
    body: { ...healthyBody, bus: { connected: false } },
  },
];

describe("when nodes report", () => {
  it("should give each node a level, an age label, and a self flag", () => {
    const rows = presenceRows(nodes, "node-a");
    expect(rows.map((r) => r.host)).toEqual(["node-a", "node-b", "node-c"]);
    expect(rows[0]).toMatchObject({ level: "ok", ageLabel: "5 s ago", isSelf: true, stale: false, gitSha: "aaa111" });
    expect(rows[0].findings).toEqual([]);
    expect(rows[1]).toMatchObject({ level: "critical", ageLabel: "10 min ago", isSelf: false, stale: true });
    expect(rows[1].findings).toEqual(["no beat for 10 min (stale past 3 intervals)"]);
    expect(rows[2]).toMatchObject({ level: "critical", isSelf: false, stale: false });
    expect(rows[2].findings).toEqual(["bus is not connected"]);
  });
});

describe("when a node has no readable time", () => {
  it("should say never and be critical", () => {
    const rows = presenceRows([{ ...nodes[0], age_secs: null, stale: true }], null);
    expect(rows[0].ageLabel).toBe("never");
    expect(rows[0].level).toBe("critical");
  });
});

describe("when ages are long", () => {
  it("should label hours and days", () => {
    const rows = presenceRows(
      [
        { ...nodes[1], host: "h", age_secs: 7_200 },
        { ...nodes[1], host: "d", age_secs: 3 * 86_400 + 100 },
      ],
      null,
    );
    expect(rows[0].ageLabel).toBe("2 h ago");
    expect(rows[1].ageLabel).toBe("3 d ago");
  });
});
