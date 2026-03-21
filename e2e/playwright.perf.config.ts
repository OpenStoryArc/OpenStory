/**
 * Playwright config for performance tests.
 *
 * Uses the mock WS server instead of Docker backend.
 * Longer timeouts for stress scenarios. Single worker to
 * avoid contention skewing measurements.
 */

import { defineConfig } from "@playwright/test";

// Perf tests use a dedicated Vite dev server on port 5199
// that proxies to the mock WS server (port set at runtime via env).
const UI_PORT = process.env.PERF_UI_PORT ? parseInt(process.env.PERF_UI_PORT) : 5199;

export default defineConfig({
  testDir: "./tests/perf",
  timeout: 120_000,
  retries: 0,
  workers: 1,
  use: {
    baseURL: `http://localhost:${UI_PORT}`,
    trace: "off",
    video: "off",
    screenshot: "only-on-failure",
    // Chromium flags for reliable perf measurement
    launchOptions: {
      args: [
        "--disable-gpu-compositing",
        "--disable-background-timer-throttling",
        "--disable-backgrounding-occluded-windows",
        "--disable-renderer-backgrounding",
      ],
    },
  },
  // The mock WS server is started inside each test file (not as a webServer)
  // because tests need to control rate, burst, disconnect, etc.
  // Vite dev server is started separately.
  webServer: {
    command: `npx vite --port ${UI_PORT} --strictPort`,
    url: `http://localhost:${UI_PORT}`,
    reuseExistingServer: true,
    cwd: "../ui",
    stdout: "pipe",
    stderr: "pipe",
    env: {
      ...process.env,
      // Mock server URL will be injected per-test via page.goto query params
      // Default to a port that the mock server will bind to
      VITE_API_URL: `http://127.0.0.1:${process.env.MOCK_WS_PORT ?? 3098}`,
    },
  },
  reporter: [
    ["list"],
    ["json", { outputFile: "test-results/perf-results.json" }],
  ],
});
