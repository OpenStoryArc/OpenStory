/**
 * Playwright global setup — starts open-story in a Docker container.
 *
 * Replaces `cargo run` in webServer config. The container loads seed data
 * from e2e/fixtures/seed-data/ and exposes port 3002 on the host.
 *
 * Requires: `docker build -t open-story:test ./rs`
 */
import path from "node:path";
import fs from "node:fs";
import { GenericContainer, Wait } from "testcontainers";

const CONTAINER_PORT = 3002;
// Match the default in playwright.config.ts — 3099 avoids the dev server on 3002
const HOST_PORT = parseInt(process.env.API_PORT ?? "3099");
const IMAGE = "open-story:test";

export default async function globalSetup() {
  console.log(`[global-setup] VITE_API_URL will be: http://localhost:${HOST_PORT}`);
  const seedDataDir = path.resolve(__dirname, "fixtures", "seed-data");

  console.log(`[global-setup] Starting open-story container (host port ${HOST_PORT})...`);

  const container = await new GenericContainer(IMAGE)
    .withExposedPorts({ container: CONTAINER_PORT, host: HOST_PORT })
    .withBindMounts([
      // Seed data is pre-translated CloudEvents JSONL — mount as /data
      // (the persistence dir), not /watch (the transcript watcher dir).
      // The SessionStore loads these on startup.
      { source: seedDataDir, target: "/data" },
    ])
    .withWaitStrategy(
      Wait.forHttp("/api/sessions", CONTAINER_PORT).forStatusCode(200),
    )
    .withStartupTimeout(30_000)
    .start();

  const apiHost = container.getHost();

  console.log(
    `[global-setup] open-story running at http://${apiHost}:${HOST_PORT}`,
  );

  // Save container ID for teardown (runs in separate worker)
  const stateFile = path.resolve(__dirname, ".container-state.json");
  fs.writeFileSync(
    stateFile,
    JSON.stringify({ containerId: container.getId() }),
  );

  // SessionStore loads seed data synchronously on startup.
  // The HTTP wait strategy already confirmed the server is ready.
}
