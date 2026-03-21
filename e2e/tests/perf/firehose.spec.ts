/**
 * Firehose performance tests — push events through the UI and measure.
 *
 * Phase 2 of Story 042: validates the mock WS server + perf sampler
 * harness works, then establishes baseline throughput metrics.
 *
 * These tests use the mock WS server (no Docker backend needed).
 * The Vite dev server proxies /ws to the mock server.
 */

import { test, expect, type Page } from "@playwright/test";
import { createMockServer, type MockServer } from "../../lib/ws-mock-server";
import { PerfSampler, type PerfReport } from "../../lib/perf-sampler";

// ═══════════════════════════════════════════════════════════════════
// Test setup — mock server lifecycle
// ═══════════════════════════════════════════════════════════════════

let mockServer: MockServer;

// The mock server port must match VITE_API_URL in playwright.perf.config.ts
const MOCK_PORT = parseInt(process.env.MOCK_WS_PORT ?? "3098");

test.beforeEach(async () => {
  mockServer = await createMockServer({
    port: MOCK_PORT,
    initialRecords: 200,
    sessions: 3,
    seed: 42,
    autoStream: false,
  });
});

test.afterEach(async () => {
  await mockServer.close();
});

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

async function waitForTimelineRows(page: Page, minRows = 1) {
  const rows = page.getByTestId("timeline-row");
  await expect(rows.first()).toBeVisible({ timeout: 15_000 });
  await expect(async () => {
    const count = await rows.count();
    expect(count).toBeGreaterThanOrEqual(minRows);
  }).toPass({ timeout: 10_000 });
  return rows;
}

/** Extract event count from status bar text like "200 events from 3 sessions". */
async function getEventCount(page: Page): Promise<number> {
  const text = await page.locator("text=/\\d+ events/").first().textContent();
  if (!text) return 0;
  const match = text.match(/(\d+)\s+events/);
  return match ? parseInt(match[1], 10) : 0;
}

/** Wait until event count reaches at least minCount. */
async function waitForEventCount(page: Page, minCount: number, timeoutMs = 15_000) {
  await expect(async () => {
    const count = await getEventCount(page);
    expect(count).toBeGreaterThanOrEqual(minCount);
  }).toPass({ timeout: timeoutMs });
}

function logReport(label: string, report: PerfReport) {
  console.log(`\n=== ${label} ===`);
  console.log(`  Duration:      ${(report.durationMs / 1000).toFixed(1)}s`);
  console.log(`  Samples:       ${report.samples.length}`);
  console.log(`  Heap growth:   ${report.heapGrowthMB.toFixed(1)} MB`);
  console.log(`  Peak heap:     ${report.peakHeapMB.toFixed(1)} MB`);
  console.log(`  DOM growth:    ${report.domNodeGrowth} nodes`);
  console.log(`  Peak DOM:      ${report.peakDomNodes} nodes`);
  console.log(`  Avg FPS:       ${report.avgFps.toFixed(0)}`);
  console.log(`  Min FPS:       ${report.minFps}`);
  console.log(`  Long tasks:    ${report.longTaskCount}`);
  console.log(`  CLS:           ${report.cls.toFixed(3)}`);
  if (report.samples.length > 0) {
    const last = report.samples[report.samples.length - 1];
    console.log(`  Final rows:    ${last.timelineRows}`);
  }
}

// ═══════════════════════════════════════════════════════════════════
// 1. Harness validation — does the mock server + UI connection work?
// ═══════════════════════════════════════════════════════════════════

test.describe("harness validation", () => {
  test("mock server sends initial_state and UI renders rows", async ({ page }) => {
    // Given: mock server with 200 initial records
    // When: navigate to UI
    await page.goto("/");

    // Then: timeline rows appear and status shows 200 events
    await waitForTimelineRows(page, 10);
    await waitForEventCount(page, 200);
    const count = await getEventCount(page);
    console.log(`Harness OK: ${count} events rendered from initial_state`);
  });

  test("streaming enriched messages increases event count", async ({ page }) => {
    await page.goto("/");
    await waitForEventCount(page, 200);

    // Given: 200 initial events
    const before = await getEventCount(page);

    // When: burst 50 enriched messages (1 record each)
    mockServer.burst(50);

    // Then: event count increases
    await waitForEventCount(page, before + 10, 10_000);
    const after = await getEventCount(page);
    expect(after).toBeGreaterThan(before);
    console.log(`Streaming OK: ${before} → ${after} events (+${after - before})`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 2. Steady-state streaming — 10 events/s for 10 seconds
// ═══════════════════════════════════════════════════════════════════

test.describe("steady-state streaming", () => {
  test("10 events/s for 10s: UI stays responsive", async ({ page }) => {
    await mockServer.close();
    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 100,
      sessions: 2,
      enrichedRate: 10,
      recordsPerMessage: 1,
      seed: 100,
      autoStream: false,
    });

    await page.goto("/");
    await waitForTimelineRows(page, 10);

    // Start sampling + streaming
    const sampler = await PerfSampler.start(page, 1000);
    mockServer.startStreaming();

    // Let it run for 10 seconds
    await page.waitForTimeout(10_000);

    mockServer.stopStreaming();
    const report = await sampler.stop();
    logReport("10 events/s × 10s", report);

    // Assertions
    expect(report.avgFps).toBeGreaterThan(30); // should be smooth
    expect(report.cls).toBeLessThan(0.5); // low layout shift
    expect(report.heapGrowthMB).toBeLessThan(50); // no runaway allocation

    // Verify events were processed
    const finalCount = await getEventCount(page);
    expect(finalCount).toBeGreaterThan(150); // 100 initial + ~100 streamed
    console.log(`Steady-state: ${finalCount} total events`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 3. Burst — 500 events at once
// ═══════════════════════════════════════════════════════════════════

test.describe("burst scenarios", () => {
  test("500-event burst: UI recovers within 5s", async ({ page }) => {
    await page.goto("/");
    await waitForTimelineRows(page, 10);

    const sampler = await PerfSampler.start(page, 500);

    // Fire burst
    mockServer.burst(500);

    // Wait for UI to process
    await waitForEventCount(page, 400, 10_000);

    const report = await sampler.stop();
    logReport("500-event burst", report);

    // The UI should have processed events
    const finalCount = await getEventCount(page);
    expect(finalCount).toBeGreaterThan(400); // 200 initial + 500 burst
    expect(report.peakHeapMB).toBeLessThan(200); // no memory explosion
    console.log(`Burst OK: ${finalCount} total events`);
  });

  test("1000-event burst: no crash, events process", async ({ page }) => {
    await page.goto("/");
    await waitForTimelineRows(page, 10);

    const sampler = await PerfSampler.start(page, 500);

    mockServer.burst(1000);
    await waitForEventCount(page, 500, 15_000);

    const report = await sampler.stop();
    logReport("1000-event burst", report);

    const finalCount = await getEventCount(page);
    expect(finalCount).toBeGreaterThan(500);
    // Main assertion: page didn't crash
    expect(await page.title()).toBeTruthy();
    console.log(`Burst 1K OK: ${finalCount} total events`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 4. Disconnect/reconnect resilience
// ═══════════════════════════════════════════════════════════════════

test.describe("disconnect/reconnect", () => {
  test("UI reconnects after server disconnect", async ({ page }) => {
    await page.goto("/");
    await waitForTimelineRows(page, 10);

    // Disconnect all clients
    mockServer.disconnectAll();
    await page.waitForTimeout(1000);

    // Server still running — UI should auto-reconnect (2s retry delay)
    await page.waitForTimeout(4000);

    // After reconnect, initial_state is resent → rows visible
    const rowsAfter = await page.getByTestId("timeline-row").count();
    expect(rowsAfter).toBeGreaterThanOrEqual(10);
    console.log(`Reconnect OK: ${rowsAfter} visible rows after reconnect`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 5. Click interaction during streaming
// ═══════════════════════════════════════════════════════════════════

test.describe("interaction under load", () => {
  test("expand/collapse rows while streaming at 10/s", async ({ page }) => {
    await mockServer.close();
    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 200,
      sessions: 2,
      enrichedRate: 10,
      seed: 200,
      autoStream: false,
    });

    await page.goto("/");
    const rows = await waitForTimelineRows(page, 20);

    mockServer.startStreaming();

    // Click to expand first 5 rows, then collapse
    for (let i = 0; i < 5; i++) {
      const row = rows.nth(i);
      await row.click();
      await page.waitForTimeout(200);
    }

    // Collapse them
    for (let i = 0; i < 5; i++) {
      const row = rows.nth(i);
      await row.click();
      await page.waitForTimeout(200);
    }

    mockServer.stopStreaming();

    // Page should still be alive and rendering
    const finalCount = await getEventCount(page);
    expect(finalCount).toBeGreaterThan(200);
    console.log(`Click under load OK: ${finalCount} events, no crash`);
  });
});
