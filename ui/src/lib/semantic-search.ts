/** Pure functions for semantic search results processing. */

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ChunkMetadata {
  record_type: string;
  timestamp: string;
  tool_name?: string;
  session_label?: string;
}

export interface SemanticSearchResult {
  event_id: string;
  session_id: string;
  score: number;
  text_snippet: string;
  metadata: ChunkMetadata;
}

export interface AgentSearchResult {
  session_id: string;
  label: string | null;
  project_id: string | null;
  project_name: string | null;
  event_count: number;
  relevance_score: number;
  matching_events: {
    event_id: string;
    score: number;
    snippet: string;
    record_type: string;
  }[];
  synopsis_url: string;
  tool_journey_url: string;
}

export interface AgentSearchResponse {
  query: string;
  results: AgentSearchResult[];
  total_events_searched: number;
}

// ---------------------------------------------------------------------------
// Display helpers (pure)
// ---------------------------------------------------------------------------

/** Format a relevance score as a percentage string. */
export function formatScore(score: number): string {
  return `${Math.round(score * 100)}%`;
}

/** Truncate a snippet to maxLen characters with ellipsis. */
export function truncateSnippet(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen).trimEnd() + "…";
}

/** Human-readable record type label. */
export function recordTypeLabel(recordType: string): string {
  const labels: Record<string, string> = {
    user_message: "User",
    assistant_message: "Assistant",
    tool_call: "Tool Call",
    tool_result: "Tool Result",
    reasoning: "Thinking",
    error: "Error",
    system_event: "System",
  };
  return labels[recordType] ?? recordType;
}

/** Group raw search results by session, returning the top N sessions. */
export function groupBySession(
  results: SemanticSearchResult[],
  limit: number,
): AgentSearchResult[] {
  const groups = new Map<string, SemanticSearchResult[]>();
  for (const r of results) {
    const existing = groups.get(r.session_id) ?? [];
    existing.push(r);
    groups.set(r.session_id, existing);
  }

  const sessions: AgentSearchResult[] = [];
  for (const [sessionId, events] of groups) {
    const maxScore = Math.max(...events.map((e) => e.score));
    sessions.push({
      session_id: sessionId,
      label: events[0]?.metadata.session_label ?? null,
      project_id: null,
      project_name: null,
      event_count: 0,
      relevance_score: maxScore,
      matching_events: events.slice(0, 3).map((e) => ({
        event_id: e.event_id,
        score: e.score,
        snippet: e.text_snippet,
        record_type: e.metadata.record_type,
      })),
      synopsis_url: `/api/sessions/${sessionId}/synopsis`,
      tool_journey_url: `/api/sessions/${sessionId}/tool-journey`,
    });
  }

  sessions.sort((a, b) => b.relevance_score - a.relevance_score);
  return sessions.slice(0, limit);
}
