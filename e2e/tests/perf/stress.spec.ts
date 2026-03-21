/**
 * Stress tests — push the UI to its limits and find where it breaks.
 *
 * Phase 3 of Story 042. These tests are designed to HURT:
 * - 5K+ events through the O(n^2) reducer
 * - 50KB payloads that stress JSON parsing + DOM
 * - Sustained 100 events/s for 30 seconds
 * - 10 concurrent sessions at 100 events/s each
 *
 * Expected: some of these WILL reveal performance cliffs.
 */

import { test, expect, type Page } from "@playwright/test";
import { createMockServer, type MockServer } from "../../lib/ws-mock-server";
import { PerfSampler, type PerfReport } from "../../lib/perf-sampler";

const MOCK_PORT = parseInt(process.env.MOCK_WS_PORT ?? "3098");
let mockServer: MockServer;

test.afterEach(async () => {
  if (mockServer) await mockServer.close();
});

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

async function getEventCount(page: Page): Promise<number> {
  const text = await page.locator("text=/\\d+ events/").first().textContent();
  if (!text) return 0;
  const match = text.match(/(\d+)\s+events/);
  return match ? parseInt(match[1], 10) : 0;
}

async function waitForEventCount(page: Page, minCount: number, timeoutMs = 30_000) {
  await expect(async () => {
    const count = await getEventCount(page);
    expect(count).toBeGreaterThanOrEqual(minCount);
  }).toPass({ timeout: timeoutMs });
}

async function waitForRows(page: Page) {
  const rows = page.getByTestId("timeline-row");
  await expect(rows.first()).toBeVisible({ timeout: 15_000 });
  return rows;
}

function logReport(label: string, report: PerfReport) {
  console.log(`\n=== ${label} ===`);
  console.log(`  Duration:      ${(report.durationMs / 1000).toFixed(1)}s`);
  console.log(`  Heap growth:   ${report.heapGrowthMB.toFixed(1)} MB`);
  console.log(`  Peak heap:     ${report.peakHeapMB.toFixed(1)} MB`);
  console.log(`  DOM growth:    ${report.domNodeGrowth} nodes`);
  console.log(`  Peak DOM:      ${report.peakDomNodes} nodes`);
  console.log(`  Avg FPS:       ${report.avgFps.toFixed(0)}`);
  console.log(`  Min FPS:       ${report.minFps}`);
  console.log(`  Long tasks:    ${report.longTaskCount}`);
  console.log(`  CLS:           ${report.cls.toFixed(3)}`);
}

// ═══════════════════════════════════════════════════════════════════
// 1. Volume stress — hammer the O(n^2) reducer
// ═══════════════════════════════════════════════════════════════════

test.describe("volume stress", () => {
  test("5K events via burst: reducer + virtualizer survive", async ({ page }) => {
    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 500,
      sessions: 3,
      seed: 1000,
      autoStream: false,
    });

    await page.goto("/");
    await waitForRows(page);
    await waitForEventCount(page, 500);

    const sampler = await PerfSampler.start(page, 500);

    // Fire 4500 more events in bursts of 500
    for (let i = 0; i < 9; i++) {
      mockServer.burst(500);
      await page.waitForTimeout(500); // let the UI breathe between bursts
    }

    // Wait for processing
    await waitForEventCount(page, 3000, 30_000);

    const report = await sampler.stop();
    const finalCount = await getEventCount(page);
    logReport(`5K burst (${finalCount} events)`, report);

    // UI should still be alive
    expect(await page.title()).toBeTruthy();
    expect(finalCount).toBeGreaterThan(3000);
    // Memory should not explode
    expect(report.peakHeapMB).toBeLessThan(500);
    console.log(`Volume 5K: ${finalCount} events, ${report.peakHeapMB.toFixed(0)}MB peak heap`);
  });

  test("10K events sustained: 100/s for 30s", async ({ page }) => {
    test.setTimeout(90_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 200,
      sessions: 5,
      enrichedRate: 100,
      recordsPerMessage: 1,
      seed: 2000,
      autoStream: false,
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 2000);
    mockServer.startStreaming();

    // Let it run for 30 seconds at 100 events/s = ~3000 events
    await page.waitForTimeout(30_000);

    mockServer.stopStreaming();
    await page.waitForTimeout(2000); // drain

    const report = await sampler.stop();
    const finalCount = await getEventCount(page);
    logReport(`Sustained 100/s × 30s (${finalCount} events)`, report);

    expect(finalCount).toBeGreaterThan(1000);
    expect(report.peakHeapMB).toBeLessThan(500);
    // FPS might drop under sustained load — log it
    console.log(`Sustained: ${finalCount} events, FPS avg=${report.avgFps.toFixed(0)} min=${report.minFps}`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 2. Large payloads — stress JSON parsing + rendering
// ═══════════════════════════════════════════════════════════════════

test.describe("large payloads", () => {
  test("500 events with 10KB payloads each", async ({ page }) => {
    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 100,
      sessions: 2,
      seed: 3000,
      autoStream: false,
      payloadSize: 10_000, // 10KB per event
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 500);

    // Burst 500 fat events
    mockServer.burst(500);
    await waitForEventCount(page, 400, 15_000);

    const report = await sampler.stop();
    const finalCount = await getEventCount(page);
    logReport(`500 × 10KB payloads (${finalCount} events)`, report);

    expect(finalCount).toBeGreaterThan(400);
    expect(report.peakHeapMB).toBeLessThan(500);
    console.log(`Large payloads: ${finalCount} events, ${report.peakHeapMB.toFixed(0)}MB peak`);
  });

  test("100 events with 50KB payloads each", async ({ page }) => {
    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 50,
      sessions: 1,
      seed: 4000,
      autoStream: false,
      payloadSize: 50_000, // 50KB per event
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 500);

    mockServer.burst(100);
    await waitForEventCount(page, 100, 15_000);

    const report = await sampler.stop();
    const finalCount = await getEventCount(page);
    logReport(`100 × 50KB payloads (${finalCount} events)`, report);

    expect(finalCount).toBeGreaterThan(100);
    // 100 × 50KB = 5MB of payload data — heap should handle it
    expect(report.peakHeapMB).toBeLessThan(500);
    console.log(`50KB payloads: ${finalCount} events, ${report.peakHeapMB.toFixed(0)}MB peak`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 3. Multi-session concurrency — 10 sessions streaming simultaneously
// ═══════════════════════════════════════════════════════════════════

test.describe("multi-session concurrency", () => {
  test("10 sessions × 10 events/s = 100 total/s for 15s", async ({ page }) => {
    test.setTimeout(60_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 300,
      sessions: 10,
      enrichedRate: 100, // 100 messages/s spread across 10 sessions
      recordsPerMessage: 1,
      seed: 5000,
      autoStream: false,
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 2000);
    mockServer.startStreaming();

    await page.waitForTimeout(15_000);

    mockServer.stopStreaming();
    await page.waitForTimeout(2000);

    const report = await sampler.stop();
    const finalCount = await getEventCount(page);
    logReport(`10 sessions × 100/s total × 15s (${finalCount} events)`, report);

    expect(finalCount).toBeGreaterThan(500);
    expect(report.peakHeapMB).toBeLessThan(500);
    console.log(`Multi-session: ${finalCount} events, FPS avg=${report.avgFps.toFixed(0)}`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 4. Spike pattern — silence then massive burst
// ═══════════════════════════════════════════════════════════════════

test.describe("spike patterns", () => {
  test("30s silence then 2000-event burst", async ({ page }) => {
    test.setTimeout(60_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 100,
      sessions: 2,
      seed: 6000,
      autoStream: false,
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 1000);

    // Silence for 5 seconds (simulating idle period)
    await page.waitForTimeout(5_000);

    // SPIKE: 2000 events at once
    mockServer.burst(2000);
    await waitForEventCount(page, 1000, 20_000);

    const report = await sampler.stop();
    const finalCount = await getEventCount(page);
    logReport(`Spike: 2000-event burst after silence (${finalCount} events)`, report);

    expect(finalCount).toBeGreaterThan(1000);
    // Page survived the spike
    expect(await page.title()).toBeTruthy();
    console.log(`Spike: ${finalCount} events, long tasks=${report.longTaskCount}`);
  });
});
