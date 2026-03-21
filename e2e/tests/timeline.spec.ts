import { test, expect } from '@playwright/test';
import { waitForTimeline, apiBaseUrl } from './helpers';

test('page loads with header and connection status', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('Open Story')).toBeVisible();
  await expect(page.getByTestId('connection-status')).toContainText(/Connected|Connecting/, { timeout: 10_000 });
});

test('timeline shows events from seed data', async ({ page }) => {
  await page.goto('/');
  const rows = await waitForTimeline(page);
  const count = await rows.count();
  expect(count).toBeGreaterThan(0);
});

test('timeline shows events from multiple sessions', async ({ page }) => {
  await page.goto('/');
  const rows = await waitForTimeline(page);

  // Seed data has multiple sessions — check for session badges
  const badges = page.getByTestId('row-session-badge');
  await expect(badges.first()).toBeVisible();
});

test('timeline shows event count in status bar', async ({ page }) => {
  await page.goto('/');
  await waitForTimeline(page);
  await expect(page.getByTestId('timeline-status')).toContainText(/\d+ events/);
});

test('clicking a row expands detail', async ({ page }) => {
  await page.goto('/');
  const rows = await waitForTimeline(page);

  // Click the first event row's button
  await rows.first().locator('button').first().click();

  // Detail pane should appear
  const detail = page.getByTestId('row-detail');
  await expect(detail.first()).toBeVisible({ timeout: 5_000 });
});

test('clicking expanded row collapses detail', async ({ page }) => {
  await page.goto('/');
  const rows = await waitForTimeline(page);

  const firstRow = rows.first();
  const rowButton = firstRow.locator('button').first();

  // Click to expand
  await rowButton.click();
  const detail = page.getByTestId('row-detail');
  await expect(detail.first()).toBeVisible({ timeout: 5_000 });

  // Click the same row button to collapse
  await rowButton.click();
  await expect(detail).not.toBeVisible({ timeout: 5_000 });
});

test('timeline shows category badges', async ({ page }) => {
  await page.goto('/');
  await waitForTimeline(page);

  // Should have at least one recognizable category badge via data-testid
  const badges = page.getByTestId('row-category-badge');
  const count = await badges.count();
  expect(count).toBeGreaterThan(0);

  // Check that at least one known category is present
  const categories = ['Prompt', 'Response', 'Tool', 'Result', 'Thinking', 'System'];
  let found = false;
  for (const cat of categories) {
    const badge = badges.filter({ hasText: cat });
    if (await badge.count() > 0) {
      found = true;
      break;
    }
  }
  expect(found).toBe(true);
});

test('hooks API accepts POST requests', async ({ page }) => {
  const res = await page.request.post(`${apiBaseUrl}/hooks`, {
    data: {
      session_id: 'e2e-hook-test',
      type: 'PostToolUse',
      event: { session_id: 'e2e-hook-test' },
    },
  });

  // 202 Accepted (no transcript file to read — expected in container)
  expect(res.status()).toBe(202);
  const body = await res.json();
  expect(body.status).toBeDefined();
});
