/**
 * Chaos tests — interactive torture while events stream.
 *
 * Phase 3 of Story 042. The UI must remain usable while being hammered.
 * - Rapid filter switching during streaming
 * - Click storm on rows during firehose
 * - Scroll slam during streaming
 * - Resize storm during streaming
 * - Repeated disconnect/reconnect cycles
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
  console.log(`  Avg FPS:       ${report.avgFps.toFixed(0)}`);
  console.log(`  Min FPS:       ${report.minFps}`);
  console.log(`  Long tasks:    ${report.longTaskCount}`);
  console.log(`  CLS:           ${report.cls.toFixed(3)}`);
}

// All filters with data-testid
const FILTER_NAMES = [
  "all", "narrative", "user", "tools", "reading", "editing", "thinking",
  "deep", "bash.git", "bash.test", "bash.build", "bash.docker",
  "compile_error", "test_pass", "test_fail", "file_create", "errors",
];

// ═══════════════════════════════════════════════════════════════════
// 1. Rapid filter switching during streaming
// ═══════════════════════════════════════════════════════════════════

test.describe("filter chaos", () => {
  test("cycle through all 17 filters 3x while streaming at 50/s", async ({ page }) => {
    test.setTimeout(60_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 300,
      sessions: 3,
      enrichedRate: 50,
      seed: 7000,
      autoStream: false,
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 1000);
    mockServer.startStreaming();

    // Cycle through all filters 3 times
    for (let cycle = 0; cycle < 3; cycle++) {
      for (const filter of FILTER_NAMES) {
        const btn = page.getByTestId(`filter-${filter}`);
        if (await btn.isVisible()) {
          await btn.click();
          await page.waitForTimeout(100); // 100ms between clicks = rapid
        }
      }
    }

    mockServer.stopStreaming();
    await page.waitForTimeout(1000);

    const report = await sampler.stop();
    const finalCount = await getEventCount(page);
    logReport(`Filter chaos: 3 × 17 filters @ 50/s (${finalCount} events)`, report);

    // Page should not have crashed
    expect(await page.title()).toBeTruthy();
    // We should have accumulated events
    expect(finalCount).toBeGreaterThan(300);
    console.log(`Filter chaos OK: ${finalCount} events, CLS=${report.cls.toFixed(3)}, long tasks=${report.longTaskCount}`);
  });

  test("toggle same filter on/off rapidly 50x during streaming", async ({ page }) => {
    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 200,
      sessions: 2,
      enrichedRate: 30,
      seed: 7100,
      autoStream: false,
    });

    await page.goto("/");
    await waitForRows(page);

    mockServer.startStreaming();

    // Hammer the "errors" filter on/off 50 times
    const errorsBtn = page.getByTestId("filter-errors");
    const allBtn = page.getByTestId("filter-all");
    for (let i = 0; i < 50; i++) {
      await errorsBtn.click();
      await page.waitForTimeout(50);
      await allBtn.click();
      await page.waitForTimeout(50);
    }

    mockServer.stopStreaming();
    expect(await page.title()).toBeTruthy();
    console.log("Toggle filter 50x OK: no crash");
  });
});

// ═══════════════════════════════════════════════════════════════════
// 2. Click storm — expand/collapse rows while firehose streams
// ═══════════════════════════════════════════════════════════════════

test.describe("click storm", () => {
  test("rapid expand/collapse 20 rows while streaming at 50/s", async ({ page }) => {
    test.setTimeout(60_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 200,
      sessions: 2,
      enrichedRate: 50,
      seed: 8000,
      autoStream: false,
    });

    await page.goto("/");
    const rows = await waitForRows(page);

    const sampler = await PerfSampler.start(page, 500);
    mockServer.startStreaming();

    // Click 20 rows rapidly to expand
    const clickCount = Math.min(20, await rows.count());
    for (let i = 0; i < clickCount; i++) {
      try {
        await rows.nth(i).click({ timeout: 2000 });
        await page.waitForTimeout(100);
      } catch {
        // Row may have shifted due to streaming — keep going
      }
    }

    // Now collapse them all
    for (let i = clickCount - 1; i >= 0; i--) {
      try {
        await rows.nth(i).click({ timeout: 2000 });
        await page.waitForTimeout(100);
      } catch {
        // Row may have shifted
      }
    }

    mockServer.stopStreaming();
    const report = await sampler.stop();
    logReport(`Click storm: 20 expand/collapse @ 50/s`, report);

    expect(await page.title()).toBeTruthy();
    console.log(`Click storm OK: CLS=${report.cls.toFixed(3)}, long tasks=${report.longTaskCount}`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 3. Scroll slam — rapid scrolling during streaming
// ═══════════════════════════════════════════════════════════════════

test.describe("scroll slam", () => {
  test("scroll top/bottom/random 20x while streaming at 30/s", async ({ page }) => {
    test.setTimeout(60_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 500,
      sessions: 3,
      enrichedRate: 30,
      seed: 9000,
      autoStream: false,
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 500);
    mockServer.startStreaming();

    // Get the scrollable container
    const scrollContainer = page.locator("[data-testid='timeline-row']").first().locator("..").locator("..");

    for (let i = 0; i < 20; i++) {
      const action = i % 4;
      switch (action) {
        case 0: // Scroll to top
          await page.evaluate(() => {
            const el = document.querySelector("[style*='overflow']") as HTMLElement;
            if (el) el.scrollTop = 0;
          });
          break;
        case 1: // Scroll to bottom
          await page.evaluate(() => {
            const el = document.querySelector("[style*='overflow']") as HTMLElement;
            if (el) el.scrollTop = el.scrollHeight;
          });
          break;
        case 2: // Scroll to middle
          await page.evaluate(() => {
            const el = document.querySelector("[style*='overflow']") as HTMLElement;
            if (el) el.scrollTop = el.scrollHeight / 2;
          });
          break;
        case 3: // Mouse wheel burst
          await page.mouse.wheel(0, 500);
          break;
      }
      await page.waitForTimeout(200);
    }

    mockServer.stopStreaming();
    const report = await sampler.stop();
    logReport("Scroll slam: 20 jumps @ 30/s", report);

    expect(await page.title()).toBeTruthy();
    console.log(`Scroll slam OK: FPS avg=${report.avgFps.toFixed(0)} min=${report.minFps}`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 4. Resize storm — programmatic viewport changes during streaming
// ═══════════════════════════════════════════════════════════════════

test.describe("resize storm", () => {
  test("resize viewport 10x while streaming at 30/s", async ({ page }) => {
    test.setTimeout(60_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 200,
      sessions: 2,
      enrichedRate: 30,
      seed: 10000,
      autoStream: false,
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 500);
    mockServer.startStreaming();

    const viewports = [
      { width: 1920, height: 1080 },
      { width: 800, height: 600 },
      { width: 2560, height: 1440 },
      { width: 1024, height: 768 },
      { width: 1366, height: 768 },
      { width: 3840, height: 2160 },
      { width: 1280, height: 800 },
      { width: 1920, height: 1080 },
      { width: 640, height: 480 },
      { width: 1920, height: 1080 },
    ];

    for (const vp of viewports) {
      await page.setViewportSize(vp);
      await page.waitForTimeout(500);
    }

    mockServer.stopStreaming();
    const report = await sampler.stop();
    logReport("Resize storm: 10 viewport changes @ 30/s", report);

    expect(await page.title()).toBeTruthy();
    // Virtual list should recalculate — verify rows still visible
    const visibleRows = await page.getByTestId("timeline-row").count();
    expect(visibleRows).toBeGreaterThan(0);
    console.log(`Resize storm OK: ${visibleRows} visible rows, CLS=${report.cls.toFixed(3)}`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 5. Disconnect storm — repeated connect/disconnect cycles
// ═══════════════════════════════════════════════════════════════════

test.describe("disconnect storm", () => {
  test("5 disconnect/reconnect cycles during streaming", async ({ page }) => {
    test.setTimeout(60_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 200,
      sessions: 2,
      enrichedRate: 20,
      seed: 11000,
      autoStream: true,
    });

    await page.goto("/");
    await waitForRows(page);

    const sampler = await PerfSampler.start(page, 1000);

    for (let i = 0; i < 5; i++) {
      // Stream for 2 seconds
      await page.waitForTimeout(2000);
      // Disconnect
      mockServer.disconnectAll();
      await page.waitForTimeout(1000);
      // UI should reconnect within 2-3 seconds
      await page.waitForTimeout(3000);
    }

    const report = await sampler.stop();
    logReport("Disconnect storm: 5 cycles", report);

    // After all cycles, rows should still be visible
    const visibleRows = await page.getByTestId("timeline-row").count();
    expect(visibleRows).toBeGreaterThan(0);
    expect(await page.title()).toBeTruthy();
    console.log(`Disconnect storm OK: ${visibleRows} rows after 5 cycles`);
  });
});

// ═══════════════════════════════════════════════════════════════════
// 6. Combined chaos — everything at once
// ═══════════════════════════════════════════════════════════════════

test.describe("combined chaos", () => {
  test("filter + click + scroll + streaming simultaneously", async ({ page }) => {
    test.setTimeout(90_000);

    mockServer = await createMockServer({
      port: MOCK_PORT,
      initialRecords: 300,
      sessions: 5,
      enrichedRate: 50,
      seed: 12000,
      autoStream: false,
    });

    await page.goto("/");
    const rows = await waitForRows(page);

    const sampler = await PerfSampler.start(page, 1000);
    mockServer.startStreaming();

    // 30 seconds of chaos
    const startTime = Date.now();
    let iteration = 0;
    while (Date.now() - startTime < 20_000) {
      iteration++;
      const action = iteration % 5;

      try {
        switch (action) {
          case 0: {
            // Click a random filter
            const filterIdx = iteration % FILTER_NAMES.length;
            const btn = page.getByTestId(`filter-${FILTER_NAMES[filterIdx]}`);
            if (await btn.isVisible()) await btn.click();
            break;
          }
          case 1: {
            // Click a row
            const rowCount = await rows.count();
            if (rowCount > 0) {
              await rows.nth(iteration % rowCount).click({ timeout: 1000 });
            }
            break;
          }
          case 2: {
            // Scroll
            await page.mouse.wheel(0, (iteration % 2 === 0) ? 300 : -300);
            break;
          }
          case 3: {
            // Fire a burst
            mockServer.burst(10);
            break;
          }
          case 4: {
            // Brief pause
            await page.waitForTimeout(100);
            break;
          }
        }
      } catch {
        // Interactions may fail if UI is busy — that's OK, keep going
      }

      await page.waitForTimeout(100);
    }

    mockServer.stopStreaming();
    await page.waitForTimeout(2000);

    const report = await sampler.stop();
    const finalCount = await getEventCount(page);
    logReport(`Combined chaos: 20s (${finalCount} events)`, report);

    // The only thing that matters: the page is still alive
    expect(await page.title()).toBeTruthy();
    const visibleRows = await page.getByTestId("timeline-row").count();
    expect(visibleRows).toBeGreaterThan(0);
    console.log(`Combined chaos SURVIVED: ${finalCount} events, ${visibleRows} visible rows, FPS=${report.avgFps.toFixed(0)}`);
  });
});
