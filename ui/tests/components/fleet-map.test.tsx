/** FleetMap — the Fleet tab: each host's beat age, build, verdict, and
 *  the consistency findings against this node, from GET /api/fleet/presence
 *  and GET /api/consistency (C-07). Its own tab, reached at #/fleet. */

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { FleetMap } from "@/components/fleet/FleetMap";
import { TabBar } from "@/components/layout/TabBar";
import { buildHash, parseHash } from "@/lib/hash-route";
import { switchTabRoute } from "@/lib/navigation";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const body = {
  status: "ok",
  boot: { phase: "serving" },
  bus: { connected: true },
  leaf: { configured: false, connected: false, hub: null },
  streams: [],
  consumers: {},
  watchers_detail: [],
  verdict: { level: "ok", findings: [] },
};

const presence = {
  interval_secs: 15,
  stale_after_secs: 45,
  nodes: [
    {
      host: "node-a",
      principal_id: "dev",
      person_id: "person-1",
      time: "2026-09-25T12:00:00.000Z",
      age_secs: 5,
      stale: false,
      status: "ok",
      git_sha: "aaa111",
      version: "0.9.0",
      body,
    },
    {
      host: "node-b",
      principal_id: "hub",
      person_id: "person-1",
      time: "2026-09-25T11:59:50.000Z",
      age_secs: 15,
      stale: false,
      status: "ok",
      git_sha: "bbb222",
      version: "0.9.1",
      body,
    },
  ],
};

const consistency = {
  host: "node-a",
  level: "warn",
  findings: [
    { id: "diverged:node-b", level: "warn", text: "node-b differs on 1 project of 3; converge would union and re-fold" },
  ],
  peers: [{ host: "node-b", compared: true, differing_projects: 1, stale: false, age_secs: 15 }],
};

function stubBoth() {
  const fetchMock = vi.fn((url: string) => {
    if (url === "/api/fleet/presence") {
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(presence) });
    }
    if (url === "/api/consistency") {
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(consistency) });
    }
    return Promise.reject(new Error(`unexpected fetch ${url}`));
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

describe("when two hosts report and one diverged", () => {
  it("should show each host's beat age, build, verdict, and the finding on the diverged host", async () => {
    const fetchMock = stubBoth();
    render(<FleetMap />);

    const a = await waitFor(() => screen.getByTestId("fleet-host-node-a"));
    expect(a).toHaveTextContent("node-a");
    expect(a).toHaveTextContent("self");
    expect(a).toHaveTextContent("5 s ago");
    expect(a).toHaveTextContent("aaa111");
    expect(a.querySelector("[data-testid='fleet-dot']")?.getAttribute("data-level")).toBe("ok");
    expect(a.querySelectorAll("[data-testid='fleet-finding']").length).toBe(0);

    const b = screen.getByTestId("fleet-host-node-b");
    expect(b).toHaveTextContent("15 s ago");
    expect(b).toHaveTextContent("bbb222");
    expect(b.querySelector("[data-testid='fleet-dot']")?.getAttribute("data-level")).toBe("warn");
    const findings = b.querySelectorAll("[data-testid='fleet-finding']");
    expect(findings.length).toBe(1);
    expect(findings[0]).toHaveTextContent("diverged:node-b");
    expect(findings[0]).toHaveTextContent("1 project");

    // The report's own level heads the tab.
    expect(screen.getByTestId("fleet-map")).toHaveTextContent("warn");

    const urls = new Set(fetchMock.mock.calls.map((c) => c[0]));
    expect(urls).toEqual(new Set(["/api/fleet/presence", "/api/consistency"]));
  });
});

describe("when the endpoints are unreachable", () => {
  it("should say so and keep the tab", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("connection refused"))));
    render(<FleetMap />);
    await waitFor(() => expect(screen.getByTestId("fleet-map")).toHaveTextContent("connection refused"));
  });
});

describe("when the Fleet tab is opened", () => {
  it("should be its own tab at #/fleet", () => {
    render(<TabBar active="live" onSwitch={() => {}} />);
    expect(screen.getByTestId("tab-fleet")).toHaveTextContent("Fleet");
    expect(parseHash("#/fleet")).toEqual({ view: "fleet" });
    expect(buildHash({ view: "fleet" })).toBe("#/fleet");
    expect(switchTabRoute({ view: "live", sessionId: "s1" }, "fleet")).toEqual({ view: "fleet" });
  });
});
