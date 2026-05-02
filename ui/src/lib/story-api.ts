/**
 * Story data fetching — REST-first with WebSocket augmentation.
 *
 * Pure functions for fetching and merging story data from the API.
 * No React, no side effects beyond fetch(). Testable in isolation.
 */

import type { PatternView } from "@/types/wire-record";

/** Session summary from the /api/sessions endpoint. */
export interface StorySession {
  session_id: string;
  label?: string | null;
  branch?: string | null;
  status?: string;
  start_time?: string;
  /** Most recent event timestamp. Sessions arrive sorted by this (DESC) from the API. */
  last_event?: string | null;
  event_count?: number;
  total_input_tokens?: number;
  total_output_tokens?: number;
  first_prompt?: string | null;
  project_name?: string | null;
  /** Origin host (machine identity) — `null` for legacy sessions ingested before host stamping. */
  host?: string | null;
  /** Origin user (human identity) — `null` for legacy sessions ingested before user stamping. */
  user?: string | null;
}

/** Response shape from GET /api/sessions */
export interface SessionsResponse {
  sessions: StorySession[];
  total: number;
}

/** Raw pattern shape from the REST API (different field names than PatternView). */
interface ApiPattern {
  pattern_type: string;
  session_id: string;
  event_ids: string[];
  started_at: string;
  ended_at: string;
  summary: string;
  metadata: Record<string, unknown>;
}

/** Response shape from GET /api/sessions/{id}/patterns */
interface PatternsResponse {
  patterns: ApiPattern[];
}

/** Map API pattern to frontend PatternView. */
function toPatternView(p: ApiPattern): PatternView {
  return {
    type: p.pattern_type,
    label: p.summary,
    session_id: p.session_id,
    events: p.event_ids,
    metadata: p.metadata,
  };
}

/** Sort modes accepted by GET /api/sessions. */
export type SessionSort = "latest" | "active" | "tokens";

/** Options accepted by `fetchSessions`. */
export interface FetchSessionsOpts {
  limit?: number;
  /** Server-side sort. Defaults to "latest" (last_event DESC). */
  sort?: SessionSort;
  /** RFC 3339 timestamp; only sessions with last_event >= this are returned. */
  since?: string;
  baseUrl?: string;
}

/** Build the query string for /api/sessions. Exported for unit testing. */
export function buildSessionsQuery(opts: FetchSessionsOpts): string {
  const { limit = 5, sort, since } = opts;
  const params = new URLSearchParams();
  params.set("limit", String(limit));
  if (sort && sort !== "latest") params.set("sort", sort);
  if (since) params.set("since", since);
  return params.toString();
}

/** Fetch recent sessions from the API. */
export async function fetchSessions(
  opts: FetchSessionsOpts = {},
): Promise<SessionsResponse> {
  const { baseUrl = "" } = opts;
  const qs = buildSessionsQuery(opts);
  const res = await fetch(`${baseUrl}/api/sessions?${qs}`);
  if (!res.ok) throw new Error(`Failed to fetch sessions: ${res.status}`);
  return res.json();
}

/** Fetch sentence patterns for a specific session. */
export async function fetchSessionSentences(
  sessionId: string,
  baseUrl: string = "",
): Promise<PatternView[]> {
  const res = await fetch(
    `${baseUrl}/api/sessions/${sessionId}/patterns?type=turn.sentence`,
  );
  if (!res.ok) throw new Error(`Failed to fetch patterns: ${res.status}`);
  const data: PatternsResponse = await res.json();
  return data.patterns.map(toPatternView);
}

/**
 * Merge new WebSocket patterns into a cached sentence list.
 * Deduplicates by (session_id, turn number). Returns null if no new
 * sentences were added (avoids unnecessary state updates).
 */
export function mergeSentences(
  existing: readonly PatternView[],
  incoming: readonly PatternView[],
): PatternView[] | null {
  const newSentences = incoming.filter((p) => {
    if (p.type !== "turn.sentence") return false;
    const turn = (p.metadata?.turn as number) ?? -1;
    return !existing.some(
      (e) =>
        e.session_id === p.session_id &&
        ((e.metadata?.turn as number) ?? -1) === turn,
    );
  });
  if (newSentences.length === 0) return null;
  return [...existing, ...newSentences];
}
