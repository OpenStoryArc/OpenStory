import { defineConfig } from '@playwright/test';

/**
 * Lightweight config for diagnostic tests against the live dev server.
 * No Docker container, no webServer — assumes dev server running on :5173.
 */
export default defineConfig({
  testDir: './tests',
  testMatch: ['fuzzy-rendering.spec.ts', 'dpi-rendering.spec.ts'],
  timeout: 30_000,
  use: {
    baseURL: 'http://localhost:5173',
    trace: 'retain-on-failure',
  },
  outputDir: './test-results',
});
