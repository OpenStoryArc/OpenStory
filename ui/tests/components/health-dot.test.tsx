/** HealthDot — the header's one-glance node status (H-08): green, amber, or
 *  red from /api/health, with the findings and the JSON one click away. */

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { HealthDot } from "@/components/layout/HealthDot";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function stubHealth(body: unknown, status = 200) {
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve({ ok: status < 400, status, json: () => Promise.resolve(body) })),
  );
}

const critical = {
  status: "ok",
  boot: { phase: "serving", replay: { done: 0, total: 0, elapsed_ms: 0 } },
  bus: { connected: true },
  leaf: { configured: true, connected: false, hub: "debian-16gb-ash-1:7422" },
  streams: [],
  consumers: {},
  watchers_detail: [],
  publish_failures: 0,
};

describe("when health has a critical", () => {
  it("should show red with the reason", async () => {
    stubHealth(critical);
    render(<HealthDot />);
    const dot = await waitFor(() => screen.getByTestId("health-dot"));
    await waitFor(() => expect(dot.getAttribute("data-level")).toBe("critical"));
    expect(dot.getAttribute("title")).toContain("leaf to debian-16gb-ash-1:7422 is not connected");

    fireEvent.click(dot);
    const panel = screen.getByTestId("health-panel");
    expect(panel).toHaveTextContent("leaf to debian-16gb-ash-1:7422 is not connected");
    expect(panel).toHaveTextContent('"connected": false', { normalizeWhitespace: true });
  });
});

describe("when health is fine", () => {
  it("should show green and say so", async () => {
    stubHealth({ ...critical, leaf: { configured: false, connected: false, hub: null } });
    render(<HealthDot />);
    const dot = await waitFor(() => screen.getByTestId("health-dot"));
    await waitFor(() => expect(dot.getAttribute("data-level")).toBe("ok"));
    expect(dot.getAttribute("title")).toBe("node health: ok");
  });
});

describe("when health cannot be fetched", () => {
  it("should show red with an unreachable finding", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new Error("connection refused"))));
    render(<HealthDot />);
    const dot = await waitFor(() => screen.getByTestId("health-dot"));
    await waitFor(() => expect(dot.getAttribute("data-level")).toBe("critical"));
    expect(dot.getAttribute("title")).toContain("health endpoint unreachable");
  });
});
