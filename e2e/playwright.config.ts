import path from 'node:path';
import { defineConfig } from '@playwright/test';

// E2E uses port 3099 by default to avoid conflicting with the dev server on 3002
const API_PORT = process.env.API_PORT ? parseInt(process.env.API_PORT) : 3099;
const UI_PORT = process.env.UI_PORT ? parseInt(process.env.UI_PORT) : 5188;

// Convert Windows path to Docker-compatible format (forward slashes, no UNC prefix)
const seedDataDir = path.resolve(__dirname, 'fixtures', 'seed-data')
  .replace(/^\\\\\?\\/, '')
  .replace(/\\/g, '/');

export default defineConfig({
  testDir: './tests',
  timeout: 30_000,
  retries: process.env.CI ? 2 : 0,
  use: {
    baseURL: `http://localhost:${UI_PORT}`,
    trace: 'on-first-retry',
  },
  webServer: [
    // 1. API server via Docker container
    {
      command: `docker run --rm --name arc-e2e-server -p ${API_PORT}:3002 -v "${seedDataDir}:/data" open-story:test`,
      url: `http://127.0.0.1:${API_PORT}/api/sessions`,
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
      stdout: 'pipe',
      stderr: 'pipe',
      env: {
        ...process.env,
        // Prevents Git Bash (MSYS) from mangling /data to C:\Program Files\Git\data
        MSYS_NO_PATHCONV: '1',
      },
    },
    // 2. Vite dev server (proxies /api and /ws to the container)
    {
      command: `npx vite --port ${UI_PORT} --strictPort`,
      url: `http://localhost:${UI_PORT}`,
      reuseExistingServer: !process.env.CI,
      cwd: '../ui',
      stdout: 'pipe',
      stderr: 'pipe',
      env: {
        ...process.env,
        VITE_API_URL: `http://localhost:${API_PORT}`,
      },
    },
  ],
});
