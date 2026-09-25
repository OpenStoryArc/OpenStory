/** fleet-map — the Fleet tab's rows from GET /api/fleet/presence and
 *  GET /api/consistency (C-07). Pure: two bodies in, one row per host out,
 *  each with its beat age, build, verdict, and the consistency findings
 *  against this node. */

import { describe, expect, it } from "vitest";
import { scenario } from "../bdd";
import {
  fleetMapRows,
  findingsFor,
  type ConsistencyReport,
  type FleetMapRow,
} from "@/lib/fleet-map";
import type { FleetPresence, PresenceNode } from "@/lib/fleet-presence";

const okBody = {
  status: "ok",
  boot: { phase: "serving" },
  bus: { connected: true },
  leaf: { configured: false, connected: false, hub: null },
  streams: [],
  consumers: {},
  watchers_detail: [],
  verdict: { level: "ok", findings: [] },
};

const nodeA: PresenceNode = {
  host: "node-a",
  principal_id: "dev",
  person_id: "person-1",
  time: "2026-09-25T12:00:00.000Z",
  age_secs: 5,
  stale: false,
  status: "ok",
  git_sha: "aaa111",
  version: "0.9.0",
  body: okBody,
};

const nodeB: PresenceNode = {
  host: "node-b",
  principal_id: "hub",
  person_id: "person-1",
  time: "2026-09-25T11:59:50.000Z",
  age_secs: 15,
  stale: false,
  status: "ok",
  git_sha: "bbb222",
  version: "0.9.1",
  body: {
    ...okBody,
    verdict: {
      level: "warn",
      findings: [
        {
          id: "stream_cap:events",
          level: "warn",
          text: "stream events is at 75% of its cap",
        },
      ],
    },
  },
};

const presence: FleetPresence = {
  interval_secs: 15,
  stale_after_secs: 45,
  nodes: [nodeA, nodeB],
};

const consistency: ConsistencyReport = {
  host: "node-a",
  level: "warn",
  findings: [
    {
      id: "diverged:node-b",
      level: "warn",
      text: "node-b differs on 1 project of 3; converge would union and re-fold",
    },
    {
      id: "lag:persist",
      level: "warn",
      text: "consumer persist has 4 batches queued behind its last receive",
    },
  ],
  peers: [
    {
      host: "node-b",
      compared: true,
      differing_projects: 1,
      stale: false,
      age_secs: 15,
    },
  ],
};

describe("when two hosts report and one diverged", () => {
  it("should attach the diverged finding to that host and this node's own findings to self", () =>
    scenario(
      () => ({ presence, consistency }),
      ({ presence, consistency }) => fleetMapRows(presence, consistency),
      (rows) => {
        expect(rows.map((r) => r.host)).toEqual(["node-a", "node-b"]);
        const [self, peer] = rows as [FleetMapRow, FleetMapRow];
        expect(self.isSelf).toBe(true);
        expect(self.consistency.map((f) => f.id)).toEqual(["lag:persist"]);
        expect(peer.isSelf).toBe(false);
        expect(peer.consistency.map((f) => f.id)).toEqual(["diverged:node-b"]);
        expect(peer.consistency[0]?.text).toContain("1 project");
        expect(peer.differingProjects).toBe(1);
        expect(peer.compared).toBe(true);
      },
    ));

  it("should read each host's verdict from its beat and worsen it for a critical finding or a stale beat", () =>
    scenario(
      () => {
        const stale: PresenceNode = { ...nodeB, stale: true, age_secs: 600 };
        const critical: ConsistencyReport = {
          ...consistency,
          level: "critical",
          findings: [
            {
              id: "diverged:node-b",
              level: "critical",
              text: "node-b differs on 3 projects of 3",
            },
          ],
        };
        return {
          presence: { ...presence, nodes: [nodeA, stale] },
          consistency: critical,
        };
      },
      ({ presence, consistency }) => fleetMapRows(presence, consistency),
      (rows) => {
        const [self, peer] = rows as [FleetMapRow, FleetMapRow];
        expect(self.level).toBe("ok");
        expect(self.verdict).toEqual([]);
        expect(peer.verdict).toEqual(["stream events is at 75% of its cap"]);
        expect(peer.level).toBe("critical");
        expect(peer.stale).toBe(true);
        expect(peer.ageLabel).toBe("10 min ago");
      },
    ));

  it("should label age and build", () =>
    scenario(
      () => ({ presence, consistency }),
      ({ presence, consistency }) => fleetMapRows(presence, consistency),
      (rows) => {
        const [self, peer] = rows as [FleetMapRow, FleetMapRow];
        expect(self.ageLabel).toBe("5 s ago");
        expect(self.build).toBe("aaa111 (0.9.0)");
        expect(peer.build).toBe("bbb222 (0.9.1)");
        expect(peer.ageLabel).toBe("15 s ago");
      },
    ));
});

describe("when a peer beats without a roll-up", () => {
  it("should say it was not compared", () =>
    scenario(
      () => ({
        presence,
        consistency: {
          ...consistency,
          findings: [],
          level: "ok" as const,
          peers: [
            {
              host: "node-b",
              compared: false,
              differing_projects: 0,
              stale: false,
              age_secs: 15,
            },
          ],
        },
      }),
      ({ presence, consistency }) => fleetMapRows(presence, consistency),
      (rows) => {
        const peer = rows[1] as FleetMapRow;
        expect(peer.compared).toBe(false);
        expect(peer.consistency).toEqual([]);
        expect(peer.level).toBe("warn");
      },
    ));
});

describe("when findings are split by host", () => {
  it("should give host-suffixed ids to that host and the rest to self", () =>
    scenario(
      () => consistency.findings,
      (findings) => ({
        b: findingsFor(findings, "node-b", false),
        self: findingsFor(findings, "node-a", true),
      }),
      ({ b, self }) => {
        expect(b.map((f) => f.id)).toEqual(["diverged:node-b"]);
        expect(self.map((f) => f.id)).toEqual(["lag:persist"]);
      },
    ));
});
