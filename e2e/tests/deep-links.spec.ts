import { test, expect } from '@playwright/test';
import { waitForTimeline, expandAllProjects } from './helpers';

test.describe('deep-link navigation', () => {
  test('#/explore opens Explore tab', async ({ page }) => {
    await page.goto('/#/explore');
    await expect(page.getByTestId('tab-explore')).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByTestId('explore-view')).toBeVisible();
  });

  test('legacy #/overview aliases onto Explore with filters intact', async ({ page }) => {
    await page.goto('/#/overview?project=OpenStory&sort=events');
    await expect(page.getByTestId('tab-explore')).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByTestId('explore-view')).toBeVisible();
    // The mirror canonicalizes the address bar to the new grammar.
    await expect
      .poll(async () => page.evaluate(() => window.location.hash))
      .toContain('#/explore?project=OpenStory');
  });

  test('#/live opens Live tab', async ({ page }) => {
    await page.goto('/#/live');
    await expect(page.getByTestId('tab-live')).toHaveAttribute('aria-selected', 'true');
  });

  test('empty hash defaults to Live tab', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);
    await expect(page.getByTestId('tab-live')).toHaveAttribute('aria-selected', 'true');
  });

  test('unknown route falls back to Live tab', async ({ page }) => {
    await page.goto('/#/nonexistent');
    await expect(page.getByTestId('tab-live')).toHaveAttribute('aria-selected', 'true');
  });

  test('#/search?q=test opens search in Explore', async ({ page }) => {
    await page.goto('/#/search?q=test');
    await expect(page.getByTestId('tab-explore')).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByTestId('explore-search')).toBeVisible();
  });

  test('clicking Explore tab updates hash', async ({ page }) => {
    await page.goto('/');
    await waitForTimeline(page);

    await page.getByTestId('tab-explore').click();
    await expect(page).toHaveURL(/#\/explore/);
  });
});
