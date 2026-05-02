/**
 * /api/users — per-user activity surface (Users tab v0.1).
 *
 * Aggregates SessionRow rows by the `user` field added in PR #42.
 * Sessions with `user: null` (legacy / pre-stamping) are excluded
 * from per-user entries — same posture as the `?user=` filter on
 * /api/sessions: a Users tab shouldn't invent an "Unknown" bucket.
 *
 * v0.1 uses the most-recent session label as the "what they're doing"
 * surface. v1 (once the InsightExtraction consumer ships — see
 * docs/research/insight-extraction-consumer.md) swaps that for
 * structured semantic insights without changing this shape.
 */

/** A single recent session shown under each user card. */
export interface UserRecentSession {
  readonly session_id: string;
  readonly label: string | null;
  readonly last_event: string | null;
  readonly project_name: string | null;
  readonly event_count: number;
}

/** One row per distinct stamped user. */
export interface UserSummary {
  readonly user: string;
  readonly session_count: number;
  readonly hosts: readonly string[];
  readonly projects: readonly string[];
  readonly last_active: string | null;
  readonly total_input_tokens: number;
  readonly total_output_tokens: number;
  readonly recent_sessions: readonly UserRecentSession[];
}

export interface UsersResponse {
  readonly users: readonly UserSummary[];
  /** All SessionRow rows (including legacy unstamped ones). The UI can
   *  render "X stamped of Y total" if useful. */
  readonly total: number;
}

/** Fetch the per-user activity list. Throws on network or parse error. */
export async function fetchUsers(baseUrl: string = ""): Promise<UsersResponse> {
  const res = await fetch(`${baseUrl}/api/users`);
  if (!res.ok) {
    throw new Error(`Failed to fetch users: ${res.status} ${res.statusText}`);
  }
  return res.json();
}
