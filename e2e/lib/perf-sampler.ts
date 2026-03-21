/**
 * Performance sampler — collects browser metrics via CDP and page.evaluate().
 *
 * Samples: JS heap size, DOM node count, layout duration, FPS estimate,
 * long tasks, and cumulative layout shift.
 *
 * Usage:
 *   const sampler = await PerfSampler.start(page);
 *   // ... run test scenario ...
 *   const report = await sampler.stop();
 *   expect(report.heapGrowthMB).toBeLessThan(50);
 */

import type { Page, CDPSession } from "@playwright/test";

// ═══════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════

export interface PerfSample {
  readonly timestamp: number;
  readonly heapUsedMB: number;
  readonly domNodes: number;
  readonly timelineRows: number;
  readonly fps: number;
}

export interface PerfReport {
  /** All samples collected during the test. */
  readonly samples: readonly PerfSample[];
  /** Duration of the sampling period in ms. */
  readonly durationMs: number;
  /** Heap growth from first to last sample in MB. */
  readonly heapGrowthMB: number;
  /** Peak heap usage in MB. */
  readonly peakHeapMB: number;
  /** DOM node count growth. */
  readonly domNodeGrowth: number;
  /** Peak DOM node count. */
  readonly peakDomNodes: number;
  /** Average FPS across all samples. */
  readonly avgFps: number;
  /** Minimum FPS observed. */
  readonly minFps: number;
  /** Long task count (>50ms, collected via PerformanceObserver). */
  readonly longTaskCount: number;
  /** Cumulative layout shift score. */
  readonly cls: number;
}

// ═══════════════════════════════════════════════════════════════════
// Sampler
// ═══════════════════════════════════════════════════════════════════

export class PerfSampler {
  private page: Page;
  private cdp: CDPSession | null = null;
  private samples: PerfSample[] = [];
  private intervalId: ReturnType<typeof setInterval> | null = null;
  private startTime = 0;

  private constructor(page: Page) {
    this.page = page;
  }

  /** Start collecting performance samples at the given interval. */
  static async start(page: Page, intervalMs = 500): Promise<PerfSampler> {
    const sampler = new PerfSampler(page);
    sampler.startTime = Date.now();

    // Install PerformanceObserver for long tasks + CLS
    await page.evaluate(() => {
      (window as any).__perfLongTasks = 0;
      (window as any).__perfCLS = 0;
      (window as any).__perfLastFrameTime = performance.now();
      (window as any).__perfFpsAccum = [];

      // Long task observer
      if ("PerformanceObserver" in window) {
        try {
          const ltObs = new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) {
              if (entry.duration > 50) {
                (window as any).__perfLongTasks++;
              }
            }
          });
          ltObs.observe({ type: "longtask", buffered: true });
          (window as any).__perfLtObs = ltObs;
        } catch {
          // longtask not supported in all browsers
        }

        // CLS observer
        try {
          const clsObs = new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) {
              if (!(entry as any).hadRecentInput) {
                (window as any).__perfCLS += (entry as any).value ?? 0;
              }
            }
          });
          clsObs.observe({ type: "layout-shift", buffered: true });
          (window as any).__perfClsObs = clsObs;
        } catch {
          // layout-shift not supported
        }
      }

      // FPS tracker via requestAnimationFrame
      function trackFps() {
        const now = performance.now();
        const delta = now - (window as any).__perfLastFrameTime;
        if (delta > 0) {
          (window as any).__perfFpsAccum.push(1000 / delta);
        }
        (window as any).__perfLastFrameTime = now;
        (window as any).__perfRafId = requestAnimationFrame(trackFps);
      }
      (window as any).__perfRafId = requestAnimationFrame(trackFps);
    });

    // Try to get CDP session for heap metrics
    try {
      sampler.cdp = await page.context().newCDPSession(page);
    } catch {
      // CDP not available in all setups
    }

    // Start periodic sampling
    sampler.intervalId = setInterval(async () => {
      try {
        const sample = await sampler.takeSample();
        if (sample) sampler.samples.push(sample);
      } catch {
        // Page may have navigated or closed
      }
    }, intervalMs);

    return sampler;
  }

  /** Take a single performance sample. */
  private async takeSample(): Promise<PerfSample | null> {
    // Collect heap via CDP
    let heapUsedMB = 0;
    if (this.cdp) {
      try {
        const metrics = await this.cdp.send("Runtime.getHeapUsage");
        heapUsedMB = (metrics as any).usedSize / (1024 * 1024);
      } catch {
        // Fall back to performance.memory
      }
    }

    // Collect DOM + FPS + row count from page
    const browserMetrics = await this.page.evaluate(() => {
      const domNodes = document.querySelectorAll("*").length;
      const timelineRows = document.querySelectorAll("[data-testid='timeline-row']").length;

      // Compute FPS from accumulated frame deltas
      const fpsArr: number[] = (window as any).__perfFpsAccum ?? [];
      const avgFps = fpsArr.length > 0
        ? fpsArr.reduce((a: number, b: number) => a + b, 0) / fpsArr.length
        : 60;
      // Reset accumulator
      (window as any).__perfFpsAccum = [];

      // Fallback heap via performance.memory
      let heapMB = 0;
      if ((performance as any).memory) {
        heapMB = (performance as any).memory.usedJSHeapSize / (1024 * 1024);
      }

      return { domNodes, timelineRows, avgFps, heapMB };
    });

    return {
      timestamp: Date.now(),
      heapUsedMB: heapUsedMB || browserMetrics.heapMB,
      domNodes: browserMetrics.domNodes,
      timelineRows: browserMetrics.timelineRows,
      fps: Math.round(browserMetrics.avgFps),
    };
  }

  /** Stop sampling and produce a report. */
  async stop(): Promise<PerfReport> {
    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }

    // Take one final sample
    try {
      const final = await this.takeSample();
      if (final) this.samples.push(final);
    } catch {
      // ignore
    }

    // Collect long task + CLS counts
    let longTaskCount = 0;
    let cls = 0;
    try {
      const counters = await this.page.evaluate(() => {
        // Clean up observers
        if ((window as any).__perfLtObs) (window as any).__perfLtObs.disconnect();
        if ((window as any).__perfClsObs) (window as any).__perfClsObs.disconnect();
        if ((window as any).__perfRafId) cancelAnimationFrame((window as any).__perfRafId);

        return {
          longTasks: (window as any).__perfLongTasks ?? 0,
          cls: (window as any).__perfCLS ?? 0,
        };
      });
      longTaskCount = counters.longTasks;
      cls = counters.cls;
    } catch {
      // ignore
    }

    // Disconnect CDP
    if (this.cdp) {
      try { await this.cdp.detach(); } catch { /* ignore */ }
    }

    const samples = this.samples;
    const durationMs = Date.now() - this.startTime;

    if (samples.length === 0) {
      return {
        samples: [],
        durationMs,
        heapGrowthMB: 0,
        peakHeapMB: 0,
        domNodeGrowth: 0,
        peakDomNodes: 0,
        avgFps: 0,
        minFps: 0,
        longTaskCount,
        cls,
      };
    }

    const heaps = samples.map((s) => s.heapUsedMB);
    const domCounts = samples.map((s) => s.domNodes);
    const fpsSamples = samples.map((s) => s.fps).filter((f) => f > 0);

    return {
      samples,
      durationMs,
      heapGrowthMB: heaps[heaps.length - 1] - heaps[0],
      peakHeapMB: Math.max(...heaps),
      domNodeGrowth: domCounts[domCounts.length - 1] - domCounts[0],
      peakDomNodes: Math.max(...domCounts),
      avgFps: fpsSamples.length > 0
        ? fpsSamples.reduce((a, b) => a + b, 0) / fpsSamples.length
        : 0,
      minFps: fpsSamples.length > 0 ? Math.min(...fpsSamples) : 0,
      longTaskCount,
      cls,
    };
  }
}
