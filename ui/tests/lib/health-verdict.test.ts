/** health-verdict — the header dot's colour, computed from /api/health the
 *  same way scripts/node_health_probe.py computes its verdict. H-08. */

import { describe, expect, it } from "vitest";
import { verdictFor } from "@/lib/health-verdict";

const healthy = {
  status: "ok",
  boot: { phase: "serving", replay: { done: 3, total: 3, elapsed_ms: 10 } },
  bus: { connected: true },
  leaf: { configured: true, connected: true, hub: "hub:7422" },
  streams: [
    { name: "events", bytes: 100, messages: 1, max_bytes: 1000, percent: 0.1 },
    { name: "ui", bytes: 0, messages: 0, max_bytes: null, percent: null },
  ],
  consumers: { persist: { alive: true, restarts: 0, lag: 0, last_restart: null, last_exit: null } },
  watchers_detail: [{ actor: "claude-code", age_secs: 12, publish_failures: 0 }],
  publish_failures: 0,
};

describe("when every signal is fine", () => {
  it("should be ok with no findings", () => {
    expect(verdictFor(healthy)).toEqual({ level: "ok", findings: [] });
  });
});

describe("when the leaf is configured but down", () => {
  it("should be critical and say so", () => {
    const v = verdictFor({ ...healthy, leaf: { configured: true, connected: false, hub: "hub:7422" } });
    expect(v.level).toBe("critical");
    expect(v.findings).toEqual(["leaf to hub:7422 is not connected"]);
  });
});

describe("when a stream nears its cap", () => {
  it("should warn at 70 percent and go critical at 90", () => {
    const at = (percent: number) => ({
      ...healthy,
      streams: [{ name: "events", bytes: 1, messages: 1, max_bytes: 1, percent }],
    });
    expect(verdictFor(at(0.71))).toEqual({ level: "warn", findings: ["stream events at 71% of its cap"] });
    expect(verdictFor(at(0.95)).level).toBe("critical");
  });
});

describe("when the node is still replaying", () => {
  it("should warn with progress, not fail", () => {
    const v = verdictFor({ ...healthy, boot: { phase: "replaying", replay: { done: 4, total: 20, elapsed_ms: 900 } } });
    expect(v).toEqual({ level: "warn", findings: ["replaying 4 of 20 sessions"] });
  });
});

describe("when a consumer is dead or a watcher is stale", () => {
  it("should rank the worst finding", () => {
    const v = verdictFor({
      ...healthy,
      bus: { connected: false },
      consumers: { persist: { alive: false, restarts: 3, lag: 0, last_restart: "x", last_exit: "y" } },
      watchers_detail: [{ actor: "grok", age_secs: 4000, publish_failures: 2 }],
    });
    expect(v.level).toBe("critical");
    expect(v.findings).toContain("bus is not connected");
    expect(v.findings).toContain("consumer persist is not alive (3 restarts)");
    expect(v.findings).toContain("watcher grok last saw an event 4000 s ago");
    expect(v.findings).toContain("watcher grok has 2 publish failures since boot");
  });
});

describe("when a consumer is held until the node serves (B-05)", () => {
  it("should not call a pending consumer dead while replaying", () => {
    const v = verdictFor({
      ...healthy,
      boot: { phase: "replaying", replay: { done: 1, total: 4, elapsed_ms: 9 } },
      consumers: {
        persist: { alive: false, state: "pending_start", restarts: 0, lag: 0 },
        patterns: { alive: false, state: "backoff", restarts: 2, lag: 0 },
      },
    });
    expect(v.level).toBe("critical");
    expect(v.findings).toContain("consumer patterns is not alive (2 restarts)");
    expect(v.findings.some((f) => f.includes("persist"))).toBe(false);
  });

  it("should warn about a consumer still pending once the node serves", () => {
    const v = verdictFor({
      ...healthy,
      consumers: { persist: { alive: false, state: "pending_start", restarts: 0, lag: 0 } },
    });
    expect(v).toEqual({ level: "warn", findings: ["consumer persist has not started yet"] });
  });
});
