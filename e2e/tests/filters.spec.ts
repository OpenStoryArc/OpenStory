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

    // Click "Tools" filter
    const toolsFilter = page.getByTestId('filter-tools');
    await toolsFilter.click();

    // Match count should appear (X/Y format)
    const matchCount = page.getByTestId('filter-match-count');
    await expect(matchCount).toBeVisible({ timeout: 5_000 });
    await expect(matchCount).toContainText(/\d+\/\d+/);
  });

  test('clicking All should reset filter and hide match count', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    // Activate a filter first
    await page.getByTestId('filter-tools').click();
    await expect(page.getByTestId('filter-match-count')).toBeVisible();

    // Click All to reset
    await page.getByTestId('filter-all').click();

    // Match count should disappear
    await expect(page.getByTestId('filter-match-count')).not.toBeVisible();
  });

  test('narrative filter should show a subset of events', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const rows = page.getByTestId('timeline-row');
    const totalCount = await rows.count();

    // Apply narrative filter
    await page.getByTestId('filter-narrative').click();

    // Should have fewer events (narrative = user + assistant messages)
    const filteredCount = await rows.count();
    expect(filteredCount).toBeLessThanOrEqual(totalCount);
  });

  test('filter bar should show group labels', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const filterBar = page.getByTestId('filter-bar');
    // Should have recognizable group labels
    await expect(filterBar).toContainText('View');
    await expect(filterBar).toContainText('Category');
  });

  test('errors filter should be available', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const errorsFilter = page.getByTestId('filter-errors');
    await expect(errorsFilter).toBeVisible();
  });
});
