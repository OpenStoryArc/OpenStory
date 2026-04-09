import { test, expect } from '@playwright/test';
import { expandAllProjects } from './helpers';

test.describe('Explore tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/#/explore');
    await expect(page.getByTestId('explore-view')).toBeVisible();
  });

  test('sidebar lists sessions from seed data', async ({ page }) => {
    const sidebar = page.getByTestId('explore-sidebar');
    await expect(sidebar).toBeVisible();

    // Expand projects if collapsed
    await expandAllProjects(page);

    // Should have session buttons
    const sessions = sidebar.locator('[data-testid^="explore-session-"]');
    await expect(sessions.first()).toBeVisible({ timeout: 10_000 });
    const count = await sessions.count();
    expect(count).toBeGreaterThan(0);
  });

  test('clicking session loads detail view', async ({ page }) => {
    const sidebar = page.getByTestId('explore-sidebar');
    await expandAllProjects(page);

    const firstSession = sidebar.locator('[data-testid^="explore-session-"]').first();
    await expect(firstSession).toBeVisible({ timeout: 10_000 });
    await firstSession.click();

    // Detail panel should appear
    await expect(page.getByTestId('explore-detail')).toBeVisible({ timeout: 10_000 });
  });

  // `search input filters sessions` retired — the sidebar search is local
  // filtering only; the URL-updating navigation it asserts on doesn't exist
  // in the current UI. The Explore tab is slated for a full rewrite (see
  // BACKLOG: Stream Architecture), at which point search behavior will be
  // re-specified.

  test('explore sidebar shows session count or status', async ({ page }) => {
    const sidebar = page.getByTestId('explore-sidebar');
    await expect(sidebar).toBeVisible();

    // Sidebar should contain text or session elements
    await expandAllProjects(page);
    const text = await sidebar.textContent();
    expect(text!.length).toBeGreaterThan(0);
  });
});
