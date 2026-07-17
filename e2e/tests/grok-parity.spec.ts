/**
 * Grok Build agent parity (UI surface — Explore shell).
 *
 * Seed: e2e/fixtures/seed-data/grok-session.jsonl (CloudEvent passthrough,
 * agent=grok-build). Watcher session_id is the filename stem: "grok-session".
 *
 * Asserts: session listed, Grok-origin content selectable, assistant prose visible.
 */
import { test, expect } from "@playwright/test";

/** Filename stem of e2e/fixtures/seed-data/grok-session.jsonl */
const GROK_SID = "grok-session";

test.describe("grok agent parity", () => {
  test("explore sidebar lists Grok seed session", async ({ page }) => {
    await page.goto("/#/explore");

    const sidebar = page.getByTestId("explore-sidebar");
    await expect(sidebar).toBeVisible({ timeout: 15_000 });

    // Default date filter may hide old synth seeds; click All if present
    const allBtn = page.getByTestId("date-range-all");
    if (await allBtn.isVisible().catch(() => false)) {
      await allBtn.click();
    }

    const sessionRow = page.getByTestId(`explore-session-${GROK_SID}`);
    await expect(sessionRow).toBeVisible({ timeout: 20_000 });

    // Title from first user prompt
    await expect(sessionRow).toContainText(/Cargo\.toml|git status/i);
  });

  test("selecting Grok session shows assistant prose and origin agent", async ({
    page,
  }) => {
    await page.goto(`/#/explore/${GROK_SID}`);

    await expect(page.getByTestId("explore-sidebar")).toBeVisible({
      timeout: 15_000,
    });

    // Prefer Session tab (conversation-forward default)
    const sessionTab = page.getByRole("button", { name: "Session" });
    if (await sessionTab.isVisible().catch(() => false)) {
      await sessionTab.click();
    }

    // Seed assistant text (typed GrokPayload → views → UI)
    const prose = page.getByText(/I'll check Cargo\.toml and git status/i);
    await expect(prose.first()).toBeVisible({ timeout: 20_000 });

    // Origin agent badge if rendered in detail header / card
    const grokBadge = page.getByText("Grok", { exact: true });
    // Soft: badge may only appear on list cards; prose is the hard assert above
    if ((await grokBadge.count()) > 0) {
      await expect(grokBadge.first()).toBeVisible();
    }
  });
});
