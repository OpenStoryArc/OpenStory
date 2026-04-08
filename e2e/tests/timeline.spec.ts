import { test, expect } from '@playwright/test';
import { waitForTimeline } from './helpers';

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

// `timeline shows events from multiple sessions` retired — `row-session-badge`
// was a per-row session indicator that the current Timeline doesn't render
// (sessions are filtered via the sidebar instead). The "events from seed data"
// test above already covers the multi-session ingest path.

test('timeline shows event count in status bar', async ({ page }) => {
  await page.goto('/');
  await waitForTimeline(page);
  await expect(page.getByTestId('timeline-status')).toContainText(/\d+ events/);
});

// `clicking a row expands detail` and `clicking expanded row collapses detail`
// retired — `row-detail` is a UI element that doesn't exist in the current
// Timeline. Row interaction patterns will be re-specified during the stream
// architecture rewrite (see BACKLOG: Stream Architecture).

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

// `hooks API accepts POST requests` retired alongside the /hooks endpoint
// (the watcher is the sole ingestion source).
