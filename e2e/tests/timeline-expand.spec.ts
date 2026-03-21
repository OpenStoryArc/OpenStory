import { test, expect } from '@playwright/test';
import { waitForTimeline } from './helpers';

/**
 * Parse the translateY value (in pixels) from a virtualizer wrapper div's style.
 * The virtualizer positions rows with `transform: translateY(Npx)`.
 */
function parseTranslateY(transform: string | null): number {
  if (!transform) return 0;
  const match = transform.match(/translateY\((\d+(?:\.\d+)?)px\)/);
  return match ? parseFloat(match[1]!) : 0;
}

test.describe('timeline row expansion', () => {
  test('should push rows below down when expanded', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    // Virtualizer wrapper divs have data-index attributes
    const firstWrapper = page.locator('div[data-index="0"]');
    const secondWrapper = page.locator('div[data-index="1"]');

    await expect(firstWrapper).toBeVisible({ timeout: 5_000 });
    await expect(secondWrapper).toBeVisible({ timeout: 5_000 });

    // Record the second row's Y position before expansion
    const yBefore = parseTranslateY(
      await secondWrapper.getAttribute('style'),
    );

    // Click the first row button to expand it
    const firstRowButton = firstWrapper.locator('button').first();
    await firstRowButton.click();

    // The expanded detail panel should appear
    const detail = firstWrapper.getByTestId('row-detail');
    await expect(detail).toBeVisible({ timeout: 5_000 });

    // Wait for the virtualizer to remeasure after expansion
    await page.waitForTimeout(300);

    // The second row should have shifted down (greater translateY)
    const yAfterExpand = parseTranslateY(
      await secondWrapper.getAttribute('style'),
    );
    expect(yAfterExpand).toBeGreaterThan(yBefore);

    // Click the first row again to collapse
    await firstRowButton.click();
    await expect(detail).not.toBeVisible({ timeout: 5_000 });

    // Wait for the virtualizer to remeasure after collapse
    await page.waitForTimeout(300);

    // The second row should return to approximately its original position
    const yAfterCollapse = parseTranslateY(
      await secondWrapper.getAttribute('style'),
    );
    expect(yAfterCollapse).toBeCloseTo(yBefore, -1); // within ~10px
  });
});
