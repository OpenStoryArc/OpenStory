/** FleetPresence — the Fleet panel reads every node's latest beat from
 *  GET /api/fleet/presence, self included through the same path (P-05). */

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { FleetPresence } from "@/components/admin/FleetPresence";

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
};

const twoNodes = {
  interval_secs: 15,
  stale_after_secs: 45,
  nodes: [
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
      body,
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
      body,
    },
  ],
};

function stubPresence(payload: unknown) {
  const fetchMock = vi.fn((url: string) => {
    if (url === "/api/fleet/presence") {
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(payload) });
    }
    return Promise.reject(new Error(`unexpected fetch ${url}`));
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

describe("when two nodes report", () => {
  it("should list both with ages", async () => {
    const fetchMock = stubPresence(twoNodes);
    render(<FleetPresence selfHost="node-a" />);

    const a = await waitFor(() => screen.getByTestId("presence-node-node-a"));
    expect(a).toHaveTextContent("node-a");
    expect(a).toHaveTextContent("5 s ago");
    expect(a).toHaveTextContent("aaa111");
    expect(a).toHaveTextContent("self");
    expect(a.querySelector("[data-testid='presence-dot']")?.getAttribute("data-level")).toBe("ok");

    const b = screen.getByTestId("presence-node-node-b");
    expect(b).toHaveTextContent("10 min ago");
    expect(b).toHaveTextContent("stale");
    expect(b.querySelector("[data-testid='presence-dot']")?.getAttribute("data-level")).toBe("critical");
    expect(b.querySelector("[data-testid='presence-dot']")?.getAttribute("title")).toContain("no beat for 10 min");

    // One path for everyone: self is read from the same endpoint, no /api/health call.
    const urls = fetchMock.mock.calls.map((c) => c[0]);
    expect(urls.every((u) => u === "/api/fleet/presence")).toBe(true);
    expect(screen.getByTestId("fleet-presence")).toHaveTextContent("every 15 s");
  });
});

describe("when nobody has reported", () => {
  it("should say so", async () => {
    stubPresence({ interval_secs: 15, stale_after_secs: 45, nodes: [] });
    render(<FleetPresence selfHost={null} />);
    await waitFor(() => expect(screen.getByTestId("fleet-presence")).toHaveTextContent("No presence yet"));
  });
});

describe("when the endpoint is unreachable", () => {
  it("should say so and keep the panel", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("connection refused"))));
    render(<FleetPresence selfHost={null} />);
    await waitFor(() => expect(screen.getByTestId("fleet-presence")).toHaveTextContent("connection refused"));
  });
});
