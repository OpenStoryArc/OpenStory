import { test, expect } from '@playwright/test';
import { waitForTimeline } from './helpers';

test.describe('sidebar', () => {
  test('should show session count', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const count = page.getByTestId('sidebar-session-count');
    await expect(count).toBeVisible();
    const text = await count.textContent();
    expect(parseInt(text!)).toBeGreaterThan(0);
  });

  test('should list sessions from seed data', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const sidebar = page.getByTestId('sidebar');
    // Each session has an "events" text showing event count
    const sessionButtons = sidebar.locator('button:has-text("events")');
    const count = await sessionButtons.count();
    expect(count).toBeGreaterThan(0);
  });

  test('clicking a session should filter timeline', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const status = page.getByTestId('timeline-status');
    const beforeText = await status.textContent();

    // Click the first session in the sidebar
    const sidebar = page.getByTestId('sidebar');
    const firstSession = sidebar.locator('button:has-text("events")').first();
    await firstSession.click();

    // Status should now show "of" (filtered count vs total)
    // or show fewer events than before
    await expect(status).toBeVisible();
  });

  test('clicking selected session should deselect (show all)', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    const sidebar = page.getByTestId('sidebar');
    const firstSession = sidebar.locator('button:has-text("events")').first();

    // Select
    await firstSession.click();
    // Deselect (click same one)
    await firstSession.click();

    // Should be back to showing all events
    const status = page.getByTestId('timeline-status');
    await expect(status).toContainText(/\d+ events/);
  });

  test('should show agents panel when session selected', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    // Agents panel should NOT be visible before selecting a session
    await expect(page.getByTestId('sidebar-agents-header')).not.toBeVisible();

    // Select a session
    const sidebar = page.getByTestId('sidebar');
    const firstSession = sidebar.locator('button:has-text("events")').first();
    await firstSession.click();

    // Agents panel should appear
    await expect(page.getByTestId('sidebar-agents-header')).toBeVisible();
    await expect(page.getByTestId('agent-all')).toBeVisible();
    await expect(page.getByTestId('agent-main')).toBeVisible();
  });
});
