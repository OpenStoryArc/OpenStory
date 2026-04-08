/**
 * Diagnostic tests for fuzzy/overlay rendering when switching sessions.
 *
 * The user reported that clicking a session tab causes the event log to
 * overlay the whole screen with fuzzy rendering. These tests capture
 * the actual DOM state to identify the root cause.
 */

import { test, expect } from '@playwright/test';
import { waitForTimeline, waitForStableRowCount, expandAllProjects } from './helpers';

/**
 * Parse translateY(Npx) from a style string. Returns NaN if not found.
 */
function parseTranslateY(style: string | null): number {
  if (!style) return NaN;
  const match = style.match(/translateY\(([^)]+)px\)/);
  return match ? parseFloat(match[1]!) : NaN;
}

test.describe('fuzzy rendering diagnostics', () => {

  test('virtual items should have integer translateY values (no subpixel blur)', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);

    // Collect all translateY values from virtual items
    const transforms = await page.locator('div[data-index]').evaluateAll(els =>
      els.map(el => {
        const style = el.getAttribute('style') ?? '';
        const match = style.match(/translateY\(([^)]+)px\)/);
        return match ? parseFloat(match[1]!) : null;
      }).filter(v => v !== null)
    );

    expect(transforms.length).toBeGreaterThan(0);

    // Every translateY should be an integer — fractional pixels cause subpixel text blur
    const fractional = transforms.filter(v => v !== Math.floor(v));
    expect(
      fractional,
      `Found ${fractional.length} non-integer translateY values: ${fractional.slice(0, 5).join(', ')}`,
    ).toHaveLength(0);
  });

  test('timeline container should not exceed its parent bounds', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);

    // Get the timeline container and its parent's bounding rects
    const bounds = await page.evaluate(() => {
      const timeline = document.querySelector('[data-testid="timeline"]');
      const parent = timeline?.parentElement;
      if (!timeline || !parent) return null;
      const tRect = timeline.getBoundingClientRect();
      const pRect = parent.getBoundingClientRect();
      return {
        timeline: { top: tRect.top, left: tRect.left, right: tRect.right, bottom: tRect.bottom, width: tRect.width, height: tRect.height },
        parent: { top: pRect.top, left: pRect.left, right: pRect.right, bottom: pRect.bottom, width: pRect.width, height: pRect.height },
      };
    });

    expect(bounds).not.toBeNull();
    // Timeline should not overflow its parent
    expect(bounds!.timeline.left).toBeGreaterThanOrEqual(bounds!.parent.left - 1);
    expect(bounds!.timeline.right).toBeLessThanOrEqual(bounds!.parent.right + 1);
    expect(bounds!.timeline.top).toBeGreaterThanOrEqual(bounds!.parent.top - 1);
    expect(bounds!.timeline.bottom).toBeLessThanOrEqual(bounds!.parent.bottom + 1);
  });

  test('clicking a session should not cause timeline to overflow', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);
    await expandAllProjects(page);

    // Take a "before" screenshot
    await page.screenshot({ path: 'test-results/before-session-click.png', fullPage: false });

    // Find and click the first session button in the sidebar
    const sessionButtons = page.locator('[data-testid="sidebar"] button[data-testid^="session-"]');
    const sessionCount = await sessionButtons.count();
    expect(sessionCount).toBeGreaterThan(0);

    await sessionButtons.first().click();

    // Wait for the timeline to re-render
    await page.waitForTimeout(500);

    // Take an "after" screenshot
    await page.screenshot({ path: 'test-results/after-session-click.png', fullPage: false });

    // Measure DOM state after click
    const afterState = await page.evaluate(() => {
      const timeline = document.querySelector('[data-testid="timeline"]');
      const sidebar = document.querySelector('[data-testid="sidebar"]');
      if (!timeline || !sidebar) return null;

      const tRect = timeline.getBoundingClientRect();
      const sRect = sidebar.getBoundingClientRect();

      // Check for overlap: timeline should not cover sidebar
      const overlaps = tRect.left < sRect.right && tRect.right > sRect.left;

      // Check virtual container
      const virtualContainer = timeline.querySelector('div[style*="position: relative"]');
      const vRect = virtualContainer?.getBoundingClientRect();

      // Check scroll container
      const scrollContainer = timeline.querySelector('.overflow-auto');
      const scrollRect = scrollContainer?.getBoundingClientRect();

      return {
        timelineRect: { left: tRect.left, top: tRect.top, right: tRect.right, bottom: tRect.bottom, width: tRect.width, height: tRect.height },
        sidebarRect: { left: sRect.left, top: sRect.top, right: sRect.right, bottom: sRect.bottom, width: sRect.width, height: sRect.height },
        overlaps,
        virtualContainer: vRect ? { height: vRect.height, top: vRect.top, bottom: vRect.bottom } : null,
        scrollContainer: scrollRect ? { height: scrollRect.height, scrollHeight: (scrollContainer as HTMLElement).scrollHeight } : null,
        virtualItemCount: timeline.querySelectorAll('div[data-index]').length,
      };
    });

    expect(afterState).not.toBeNull();

    // Timeline should not overlap the sidebar
    expect(afterState!.overlaps, 'Timeline overlaps sidebar — layout is broken').toBe(false);

    // Timeline width should be reasonable (not stretching full viewport)
    expect(afterState!.timelineRect.left).toBeGreaterThanOrEqual(afterState!.sidebarRect.right - 1);
  });

  test('virtual items should not overlap each other', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);

    // Get all virtual item positions
    const items = await page.locator('div[data-index]').evaluateAll(els =>
      els.map(el => {
        const rect = el.getBoundingClientRect();
        const index = parseInt(el.getAttribute('data-index') ?? '-1');
        return { index, top: rect.top, bottom: rect.bottom, height: rect.height };
      }).sort((a, b) => a.top - b.top)
    );

    expect(items.length).toBeGreaterThan(1);

    // Check adjacent items for overlap (allow 1px tolerance for rounding)
    const overlaps: string[] = [];
    for (let i = 0; i < items.length - 1; i++) {
      const current = items[i]!;
      const next = items[i + 1]!;
      if (current.bottom > next.top + 1) {
        overlaps.push(
          `item[${current.index}] bottom=${current.bottom.toFixed(1)} overlaps item[${next.index}] top=${next.top.toFixed(1)}`
        );
      }
    }

    expect(
      overlaps,
      `Found ${overlaps.length} overlapping virtual items:\n${overlaps.slice(0, 5).join('\n')}`,
    ).toHaveLength(0);
  });

  test('session switch should produce correct virtualizer height', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);
    await expandAllProjects(page);

    // Get initial state
    const beforeRowCount = await page.getByTestId('timeline-row').count();

    // Click a session
    const sessionButtons = page.locator('[data-testid="sidebar"] button[data-testid^="session-"]');
    if (await sessionButtons.count() === 0) {
      test.skip();
      return;
    }
    await sessionButtons.first().click();
    await page.waitForTimeout(500);

    const afterRowCount = await page.getByTestId('timeline-row').count();

    // Virtual container height should be proportional to visible rows
    const containerInfo = await page.evaluate(() => {
      const timeline = document.querySelector('[data-testid="timeline"]');
      const virtualContainer = timeline?.querySelector('div[style*="position: relative"]');
      if (!virtualContainer) return null;
      const style = virtualContainer.getAttribute('style') ?? '';
      const heightMatch = style.match(/height:\s*(\d+(?:\.\d+)?)px/);
      const height = heightMatch ? parseFloat(heightMatch[1]!) : 0;
      const visibleRows = virtualContainer.querySelectorAll('div[data-index]').length;
      return { height, visibleRows };
    });

    expect(containerInfo).not.toBeNull();

    // The container height should be reasonable:
    // at minimum 32px * visible rows (collapsed row height)
    // at maximum some reasonable multiple
    const minExpected = containerInfo!.visibleRows * 32;
    expect(
      containerInfo!.height,
      `Virtual container height ${containerInfo!.height}px seems wrong for ${containerInfo!.visibleRows} visible rows (min expected: ${minExpected}px)`,
    ).toBeGreaterThanOrEqual(minExpected * 0.5); // allow 50% tolerance for overscan

    // Container height should not be absurdly large (stale from before filter).
    // 2x ratio is the current virtualizer's overscan + measurement headroom;
    // 4x is the "something is structurally wrong with the virtualizer" line.
    const maxReasonable = afterRowCount * 1000 + 5000; // generous: 4x expanded row height + buffer
    expect(
      containerInfo!.height,
      `Virtual container height ${containerInfo!.height}px is unreasonably large for ${afterRowCount} rows`,
    ).toBeLessThan(maxReasonable);
  });

  test('screenshot comparison: before and after session click', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await waitForStableRowCount(page, 500);
    await expandAllProjects(page);

    // Capture the timeline area before clicking
    const timelineLocator = page.getByTestId('timeline');
    await expect(timelineLocator).toBeVisible();

    const beforeShot = await timelineLocator.screenshot();

    // Click a session
    const sessionButtons = page.locator('[data-testid="sidebar"] button[data-testid^="session-"]');
    if (await sessionButtons.count() === 0) {
      test.skip();
      return;
    }
    await sessionButtons.first().click();
    await page.waitForTimeout(800); // allow render

    const afterShot = await timelineLocator.screenshot();

    // Save screenshots for manual inspection
    const fs = await import('fs');
    const dir = 'test-results';
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(`${dir}/timeline-before-session.png`, beforeShot);
    fs.writeFileSync(`${dir}/timeline-after-session.png`, afterShot);

    // The screenshots should be different (filter changed the view)
    // but both should exist and have reasonable dimensions
    expect(beforeShot.length).toBeGreaterThan(1000);
    expect(afterShot.length).toBeGreaterThan(1000);
  });
});
