/**
 * DPI/scaling rendering tests.
 *
 * Windows display scaling (125%, 150%) can cause subpixel rendering issues
 * with CSS transforms. These tests run at different device scale factors
 * to capture the actual rendering state.
 */

import { test, expect, type Page } from '@playwright/test';
import { waitForTimeline, waitForStableRowCount, expandAllProjects } from './helpers';

/**
 * Capture detailed rendering metrics at the current DPI.
 */
async function captureRenderingMetrics(page: Page) {
  return page.evaluate(() => {
    const dpr = window.devicePixelRatio;
    const timeline = document.querySelector('[data-testid="timeline"]');
    const virtualItems = Array.from(timeline?.querySelectorAll('div[data-index]') ?? []);

    // Check translateY values — at non-integer DPR, integer CSS px can
    // become fractional device px, causing blurry text
    const transforms = virtualItems.map(el => {
      const style = (el as HTMLElement).getAttribute('style') ?? '';
      const match = style.match(/translateY\(([^)]+)px\)/);
      const cssY = match ? parseFloat(match[1]!) : 0;
      const deviceY = cssY * dpr;
      return {
        index: el.getAttribute('data-index'),
        cssY,
        deviceY,
        isDevicePixelAligned: Math.abs(deviceY - Math.round(deviceY)) < 0.01,
      };
    });

    const misaligned = transforms.filter(t => !t.isDevicePixelAligned);

    // Check actual rendered positions via getBoundingClientRect
    // These reflect the real device-pixel positions
    const rects = virtualItems.map(el => {
      const r = (el as HTMLElement).getBoundingClientRect();
      return {
        index: el.getAttribute('data-index'),
        top: r.top,
        height: r.height,
        // Fractional top means the element sits between device pixels
        isFractionalTop: Math.abs(r.top - Math.round(r.top)) > 0.01,
      };
    });

    const fractionalTops = rects.filter(r => r.isFractionalTop);

    // Check the row height — should it be a multiple of device pixels?
    const rowHeights = new Set(rects.map(r => r.height));

    // Check font rendering
    const body = document.body;
    const bodyStyle = getComputedStyle(body);

    return {
      devicePixelRatio: dpr,
      virtualItemCount: virtualItems.length,
      misalignedTransforms: misaligned.length,
      misalignedExamples: misaligned.slice(0, 3).map(t => ({
        index: t.index,
        cssY: t.cssY,
        deviceY: t.deviceY.toFixed(2),
      })),
      fractionalTops: fractionalTops.length,
      fractionalTopExamples: fractionalTops.slice(0, 3).map(r => ({
        index: r.index,
        top: r.top.toFixed(2),
      })),
      uniqueRowHeights: Array.from(rowHeights).sort(),
      fontSmoothing: bodyStyle.getPropertyValue('-webkit-font-smoothing'),
      textRendering: bodyStyle.getPropertyValue('text-rendering'),
    };
  });
}

// Test at 1x (standard)
test.describe('rendering at 1x DPI', () => {
  test.use({ deviceScaleFactor: 1 });

  test('should render cleanly at 1x', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);

    const metrics = await captureRenderingMetrics(page);
    console.log('1x DPI metrics:', JSON.stringify(metrics, null, 2));

    expect(metrics.devicePixelRatio).toBe(1);
    expect(metrics.misalignedTransforms).toBe(0);

    await page.screenshot({ path: 'test-results/dpi-1x.png' });
  });
});

// Test at 1.25x (Windows 125% scaling)
test.describe('rendering at 1.25x DPI', () => {
  test.use({ deviceScaleFactor: 1.25 });

  test('should render cleanly at 1.25x', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);

    const metrics = await captureRenderingMetrics(page);
    console.log('1.25x DPI metrics:', JSON.stringify(metrics, null, 2));

    expect(metrics.devicePixelRatio).toBe(1.25);

    // At 1.25x, translateY values that aren't multiples of 0.8 will
    // produce fractional device pixels (e.g., 32px * 1.25 = 40 device px = OK,
    // but 33px * 1.25 = 41.25 device px = fuzzy)
    console.log(`Misaligned at 1.25x: ${metrics.misalignedTransforms} of ${metrics.virtualItemCount}`);
    console.log(`Fractional tops: ${metrics.fractionalTops}`);

    await page.screenshot({ path: 'test-results/dpi-1.25x.png' });
  });
});

// Test at 1.5x (Windows 150% scaling)
test.describe('rendering at 1.5x DPI', () => {
  test.use({ deviceScaleFactor: 1.5 });

  test('should render cleanly at 1.5x', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);

    const metrics = await captureRenderingMetrics(page);
    console.log('1.5x DPI metrics:', JSON.stringify(metrics, null, 2));

    expect(metrics.devicePixelRatio).toBe(1.5);

    // At 1.5x, a 32px row = 48 device px (aligned).
    // But cumulative transforms (32*N px) can drift: 32*3=96 → 96*1.5=144 (aligned)
    // The issue is when intermediate rows have odd heights from measurement
    console.log(`Misaligned at 1.5x: ${metrics.misalignedTransforms} of ${metrics.virtualItemCount}`);
    console.log(`Fractional tops: ${metrics.fractionalTops}`);

    await page.screenshot({ path: 'test-results/dpi-1.5x.png' });
  });
});

// Test session switching at 1.25x (most common Windows setting for fuzzy reports)
test.describe('session switch at 1.25x DPI', () => {
  test.use({ deviceScaleFactor: 1.25 });

  test('should not degrade rendering after session click', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);
    await expandAllProjects(page);

    await page.screenshot({ path: 'test-results/dpi-1.25x-before-click.png' });

    const sessionButtons = page.locator('[data-testid="sidebar"] button[data-testid^="session-"]');
    if (await sessionButtons.count() === 0) {
      test.skip();
      return;
    }
    await sessionButtons.first().click();
    await page.waitForTimeout(800);

    const afterMetrics = await captureRenderingMetrics(page);
    console.log('1.25x after click:', JSON.stringify(afterMetrics, null, 2));

    await page.screenshot({ path: 'test-results/dpi-1.25x-after-click.png' });

    // Key assertion: no items should have fractional bounding box tops
    // after session switch (would indicate layout thrash from stale virtualizer state)
    console.log(`Fractional tops after click: ${afterMetrics.fractionalTops} of ${afterMetrics.virtualItemCount}`);
  });
});
