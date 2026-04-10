import { test, expect } from '@playwright/test';
import { apiBaseUrl } from './helpers';

test.describe('REST API smoke tests', () => {
  test('GET /api/sessions returns session list', async ({ request }) => {
    const res = await request.get(`${apiBaseUrl}/api/sessions`);
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.sessions).toBeDefined();
    expect(Array.isArray(body.sessions)).toBe(true);
    expect(body.sessions.length).toBeGreaterThan(0);
    expect(typeof body.total).toBe('number');
  });

  test('GET /api/sessions/:id/events returns events for a session', async ({ request }) => {
    // First get the session list
    const sessRes = await request.get(`${apiBaseUrl}/api/sessions`);
    const { sessions } = await sessRes.json();
    expect(sessions.length).toBeGreaterThan(0);

    const sessionId = sessions[0].session_id;
    const eventsRes = await request.get(`${apiBaseUrl}/api/sessions/${sessionId}/events`);
    expect(eventsRes.status()).toBe(200);
    const events = await eventsRes.json();
    expect(Array.isArray(events)).toBe(true);
    expect(events.length).toBeGreaterThan(0);
  });

  test('GET /api/sessions returns sessions with expected fields', async ({ request }) => {
    const res = await request.get(`${apiBaseUrl}/api/sessions`);
    const { sessions } = await res.json();
    const first = sessions[0];
    expect(first.session_id).toBeDefined();
    expect(typeof first.session_id).toBe('string');
  });

  test('GET /api/sessions/:id/events returns events with expected fields', async ({ request }) => {
    const sessRes = await request.get(`${apiBaseUrl}/api/sessions`);
    const { sessions } = await sessRes.json();
    const sessionId = sessions[0].session_id;

    const eventsRes = await request.get(`${apiBaseUrl}/api/sessions/${sessionId}/events`);
    const events = await eventsRes.json();
    const first = events[0];
    // Events should have standard CloudEvent fields
    expect(first.id).toBeDefined();
    expect(first.type).toBeDefined();
  });

  test('GET /api/sessions/:id/events for nonexistent session returns empty or 404', async ({ request }) => {
    const res = await request.get(`${apiBaseUrl}/api/sessions/nonexistent-session-id/events`);
    // Depending on implementation: empty array or 404
    const status = res.status();
    expect([200, 404]).toContain(status);
    if (status === 200) {
      const body = await res.json();
      expect(Array.isArray(body)).toBe(true);
    }
  });

  // POST /hooks tests retired alongside the /hooks endpoint
  // (the watcher is the sole ingestion source).
});
