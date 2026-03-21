import { expect, type Page, type Locator } from '@playwright/test';

/** Base URL for API requests (Docker container). */
export const API_PORT = process.env.API_PORT ? parseInt(process.env.API_PORT) : 3099;
export const apiBaseUrl = `http://localhost:${API_PORT}`;

/**
 * Wait for the timeline to load events.
 * Returns a locator for all timeline row containers.
 */
export async function waitForTimeline(page: Page): Promise<Locator> {
  const rows = page.getByTestId('timeline-row');
  await expect(rows.first()).toBeVisible({ timeout: 15_000 });
  return rows;
}

/**
 * Wait until the timeline row count stabilizes (no new rows for `stableMs`).
 * Useful after initial load when backfill may still be streaming.
 */
export async function waitForStableRowCount(
  page: Page,
  stableMs = 500,
  timeoutMs = 10_000,
): Promise<number> {
  const rows = page.getByTestId('timeline-row');
  const start = Date.now();
  let lastCount = 0;
  let lastChange = start;

  while (Date.now() - start < timeoutMs) {
    const count = await rows.count();
    if (count !== lastCount) {
      lastCount = count;
      lastChange = Date.now();
    } else if (Date.now() - lastChange >= stableMs) {
      return lastCount;
    }
    await page.waitForTimeout(100);
  }
  return lastCount;
}

/**
 * Expand project groups in the sidebar that may be collapsed
 * (sessions older than 24h get grouped by project).
 */
export async function expandAllProjects(page: Page) {
  const projectHeaders = page.locator('[data-testid="sidebar"] button:has-text("project")');
  const count = await projectHeaders.count();
  for (let i = 0; i < count; i++) {
    await projectHeaders.nth(i).click();
  }
}

/**
 * Select a session in the sidebar by its short ID prefix.
 */
export async function selectSession(page: Page, idPrefix: string) {
  const btn = page.getByTestId(`session-${idPrefix}`);
  await btn.click();
}
