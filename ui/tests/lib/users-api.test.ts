/**
 * Spec: fetchUsers — REST client for /api/users.
 *
 * Driven by the contract documented in `lib/users-api.ts`. We don't
 * boot a real server here; we mock fetch and assert the shape the UI
 * relies on stays stable.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { fetchUsers, type UsersResponse } from "@/lib/users-api";

describe("fetchUsers", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("should parse a typical /api/users response", async () => {
    const payload: UsersResponse = {
      total: 4,
      users: [
        {
          user: "katie",
          session_count: 2,
          hosts: ["Katies-Mac-mini", "Katies-MacBook-Pro"],
          projects: ["OpenStory"],
          last_active: "2026-05-02T17:00:00Z",
          total_input_tokens: 12345,
          total_output_tokens: 67890,
          activity_24h: Array(24).fill(0).map((_, i) => i),
          recent_sessions: [
            {
              session_id: "sess-a",
              label: "fix the migration",
              last_event: "2026-05-02T17:00:00Z",
              project_name: "OpenStory",
              event_count: 87,
            },
          ],
        },
      ],
    };
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(JSON.stringify(payload), { status: 200 })),
    );

    const got = await fetchUsers();
    expect(got.total).toBe(4);
    expect(got.users).toHaveLength(1);
    expect(got.users[0]!.user).toBe("katie");
    expect(got.users[0]!.recent_sessions[0]!.session_id).toBe("sess-a");
    // activity_24h shape contract: 24 numeric buckets.
    expect(got.users[0]!.activity_24h).toHaveLength(24);
    expect(got.users[0]!.activity_24h[23]).toBe(23);
  });

  it("should accept an empty response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ users: [], total: 0 }), { status: 200 }),
      ),
    );
    const got = await fetchUsers();
    expect(got.users).toEqual([]);
    expect(got.total).toBe(0);
  });

  it("should throw on a non-200 response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("error", { status: 500 })),
    );
    await expect(fetchUsers()).rejects.toThrow(/500/);
  });
});
