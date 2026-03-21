/**
 * Playwright global teardown — stops the open-story container.
 */
import path from "node:path";
import fs from "node:fs";

export default async function globalTeardown() {
  const stateFile = path.resolve(__dirname, ".container-state.json");

  if (!fs.existsSync(stateFile)) {
    console.log("[global-teardown] No container state file — nothing to stop.");
    return;
  }

  const { containerId } = JSON.parse(fs.readFileSync(stateFile, "utf-8"));
  fs.unlinkSync(stateFile);

  if (!containerId) {
    return;
  }

  console.log(
    `[global-teardown] Stopping container ${containerId.slice(0, 12)}...`,
  );

  // Use Docker CLI to stop — simpler than re-attaching via testcontainers
  const { execSync } = await import("node:child_process");
  try {
    execSync(`docker stop ${containerId}`, { timeout: 10_000, stdio: "pipe" });
    execSync(`docker rm ${containerId}`, { timeout: 10_000, stdio: "pipe" });
    console.log("[global-teardown] Container stopped and removed.");
  } catch {
    // Container may already be stopped by testcontainers
    console.log("[global-teardown] Container already stopped.");
  }
}
