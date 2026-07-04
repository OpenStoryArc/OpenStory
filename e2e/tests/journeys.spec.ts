/** JOURNEYS — the Storm board's user journeys as executable requirements.
 *
 *  Each spec walks a whole path a sovereign user actually takes, across the
 *  edges the ActionGraph declares. Where the graph proves single hops land
 *  (scripts/nav_survey.mjs), a journey proves the hops COMPOSE.
 *
 *  Journey 1 — "read a session's story":
 *    Live → select a session → its events render → switch to Story (the
 *    session carries across tabs) → its turns render → drill a turn to its
 *    SOURCE event → land in Explore on that exact event.
 */

import { test, expect } from '@playwright/test';
import { expandAllProjects, apiBaseUrl } from './helpers';

// The smallest seed session: 19 events, 6 turn patterns.
const SESSION = 'synth-session';
// Sidebar rows use the 8-char id prefix as their testid.
const SESSION_TESTID = `session-${SESSION.slice(0, 8)}`;

test.describe('journey: read a session\'s story', () => {
  test('Live → session → Story → turn → source event', async ({ page }) => {
    // 1. Arrive on Live. The default feed is calm (initial_state carries no
    //    historical records by design) — the journey starts by CHOOSING.
    await page.goto('/#/live');
    await expandAllProjects(page);

    // 2. Select the session: its events load (REST page) and render.
    await page.getByTestId(SESSION_TESTID).click();
    await expect(page.getByTestId('timeline-row').first()).toBeVisible({ timeout: 15_000 });

    // 3. Switch tabs: the session RIDES ALONG (carry-session-across-tabs).
    await page.getByRole('tab', { name: 'Story' }).click();
    await expect(page).toHaveURL(new RegExp(`story/${SESSION}`));

    // 4. The story renders turns from the session's patterns.
    const turnCard = page.getByTestId('turn-drill-source').first();
    await expect(turnCard).toBeVisible({ timeout: 15_000 });

    // 5. Drill the turn to its source: land in Explore on the EXACT event.
    await turnCard.click();
    await expect(page).toHaveURL(new RegExp(`explore/${SESSION}/event/`));

    // 6. And the destination is real — Explore's event cards are on screen.
    await expect(page.locator('[data-event-id]').first()).toBeVisible({ timeout: 15_000 });
  });
});

/** Journey 2 — "agent drives the mirror":
 *    An agent POSTs a control action to the seam (exactly what the MCP
 *    does) → the human's browser FOLLOWS to the exact event → the drive is
 *    ATTRIBUTED on screen → and the human gets the wheel back (the banner
 *    clears; nothing keeps steering). Sovereignty: the seam only authors
 *    ui.* intents — the observed events.* stream is never touched.
 */
test.describe('journey: agent drives the mirror', () => {
  test('control POST → browser follows → attributed → wheel returns', async ({ page, request }) => {
    // A real event id from the seed session — the drive target.
    const records = await (
      await request.get(`${apiBaseUrl}/api/sessions/${SESSION}/records?limit=1`)
    ).json();
    const eventId = records[0].id as string;

    // 1. The human is parked on Overview, doing their own thing — and the
    //    mirror is live (control intents arrive over the WebSocket).
    await page.goto('/#/overview');
    await expect(page.getByTestId('connection-status')).toContainText('Connected', {
      timeout: 10_000,
    });

    // 2. An agent drives: focus_event through the control seam.
    await request.post(`${apiBaseUrl}/api/control`, {
      data: {
        action: 'focus_event',
        params: { sessionId: SESSION, eventId, view: 'explore' },
        issuer: 'e2e-agent',
      },
    });

    // 3. The browser follows to the exact event…
    await expect(page).toHaveURL(new RegExp(`explore/${SESSION}/event/${eventId}`), {
      timeout: 10_000,
    });

    // 4. …and the drive is attributed, visibly.
    await expect(page.getByTestId('driven-by')).toBeVisible();
    await expect(page.getByTestId('driven-by')).toContainText('e2e-agent');

    // 5. The wheel returns: attribution clears on its own (~4 s) — an agent
    //    can show, but never silently KEEP the helm.
    await expect(page.getByTestId('driven-by')).toBeHidden({ timeout: 8_000 });
  });
});

/** Journey 3 — "follow the user & act in rests":
 *    The human moves → the mirror records the move (authored ui.* stream) →
 *    an agent reading /api/ui-state SEES where they are and their tempo →
 *    the agent waits for a REST (active_now false after ~8 s idle) →
 *    and only then presents — the human sees the message without ever
 *    being interrupted mid-flow.
 */
test.describe('journey: follow the user & act in rests', () => {
  test('navigate → ui-state follows → rest detected → present lands', async ({ page, request }) => {
    test.slow(); // deliberately waits out the 8 s idle threshold

    // Tempo models ONE human: parallel test workers navigating their own
    // pages feed the same viewing-session stream, so a "rest" may never
    // come under full-suite load. Run this journey isolated
    // (npx playwright test journeys) — in a shared run it politely skips.
    test.skip(
      process.env.PW_JOURNEYS_ISOLATED !== '1' && !!process.env.PW_PARALLEL_SUITE,
      'tempo is a single-human model; run isolated',
    );

    // 1. The human navigates to a specific place.
    await page.goto(`/#/story/${SESSION}`);
    await expect(page.getByTestId('connection-status')).toContainText('Connected', {
      timeout: 10_000,
    });

    // 2. An agent following along sees WHERE they are (ui_state freshness).
    await expect
      .poll(
        async () => {
          const s = await (await request.get(`${apiBaseUrl}/api/ui-state`)).json();
          return s.ui_state?.view;
        },
        { timeout: 10_000 },
      )
      .toBe('story');

    // 3. …and their rhythm: they just moved, so they're ACTIVE.
    const active = await (await request.get(`${apiBaseUrl}/api/ui-state`)).json();
    expect(active.tempo.active_now).toBe(true);

    // 4. The agent waits for a rest (idle > 8 s → active_now flips false).
    await expect
      .poll(
        async () => {
          const s = await (await request.get(`${apiBaseUrl}/api/ui-state`)).json();
          return s.tempo.active_now;
        },
        { timeout: 15_000, intervals: [1000] },
      )
      .toBe(false);

    // 5. Only in the rest does the agent speak — and the human sees it.
    await request.post(`${apiBaseUrl}/api/control`, {
      data: {
        action: 'present',
        params: { message: 'While you rested: the build is green.' },
        issuer: 'e2e-agent',
      },
    });
    await expect(page.getByTestId('present-banner')).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId('present-banner')).toContainText('the build is green');
  });
});
