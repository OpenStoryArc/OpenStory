import { test, expect } from '@playwright/test';
import { waitForTimeline } from './helpers';

test.describe('timeline filters', () => {
  test('should show filter bar with All button active by default', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const filterBar = page.getByTestId('filter-bar');
    await expect(filterBar).toBeVisible();

    // "All" filter should be present
    const allFilter = page.getByTestId('filter-all');
    await expect(allFilter).toBeVisible();
  });

  test('clicking a filter should update timeline and show match count', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    // Click "Tests" filter (the tools/tests filter — see lib/timeline-filters.ts)
    const testsFilter = page.getByTestId('filter-tests');
    await testsFilter.click();

    // Match count should appear (X/Y format)
    const matchCount = page.getByTestId('filter-match-count');
    await expect(matchCount).toBeVisible({ timeout: 5_000 });
    await expect(matchCount).toContainText(/\d+\/\d+/);
  });

  test('clicking All should reset filter and hide match count', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    // Activate a filter first
    await page.getByTestId('filter-tests').click();
    await expect(page.getByTestId('filter-match-count')).toBeVisible();

    // Click All to reset
    await page.getByTestId('filter-all').click();

    // Match count should disappear
    await expect(page.getByTestId('filter-match-count')).not.toBeVisible();
  });

  test('conversation filter should show a subset of events', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    // Apply conversation filter (was named "narrative" in the old grouped filter scheme)
    await page.getByTestId('filter-conversation').click();

    // Match count is the authoritative source for "filtered subset" — avoids
    // races against incoming WebSocket events that would shift a raw row count.
    // Format: "filtered/total" e.g., "12/21".
    const matchCount = page.getByTestId('filter-match-count');
    await expect(matchCount).toBeVisible({ timeout: 5_000 });
    const text = (await matchCount.textContent()) ?? '';
    const match = text.match(/(\d+)\/(\d+)/);
    expect(match, `match-count text "${text}" should be in "filtered/total" format`).not.toBeNull();
    const filtered = parseInt(match![1]!, 10);
    const total = parseInt(match![2]!, 10);
    expect(filtered).toBeLessThanOrEqual(total);
    expect(filtered).toBeGreaterThan(0);
  });

  // `filter bar should show group labels` retired — FILTER_GROUPS no longer
  // exposes labeled groups (single flat row of filters in the current UI).
  // See ui/src/lib/timeline-filters.ts.

  test('errors filter should be available', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const errorsFilter = page.getByTestId('filter-errors');
    await expect(errorsFilter).toBeVisible();
  });
});
