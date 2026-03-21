/** Semantic search panel — queries /api/agent/search and displays session-grouped results. */

import { useState, useCallback, useEffect, useRef } from "react";
import type { AgentSearchResponse, AgentSearchResult } from "@/lib/semantic-search";
import { formatScore, truncateSnippet, recordTypeLabel } from "@/lib/semantic-search";

interface SemanticSearchProps {
  onSelectSession: (sessionId: string) => void;
  /** Pre-fill query from URL deep-link. */
  initialQuery?: string;
}

const DEBOUNCE_MS = 300;

export function SemanticSearch({ onSelectSession, initialQuery }: SemanticSearchProps) {
  const [query, setQuery] = useState(initialQuery ?? "");
  const appliedInitial = useRef(false);

  // Apply initial query once (on first mount or when it changes from undefined to a value)
  useEffect(() => {
    if (initialQuery && !appliedInitial.current) {
      appliedInitial.current = true;
      setQuery(initialQuery);
    }
  }, [initialQuery]);
  const [results, setResults] = useState<AgentSearchResult[]>([]);
  const [totalSearched, setTotalSearched] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const search = useCallback((q: string) => {
    if (!q.trim()) {
      setResults([]);
      setTotalSearched(0);
      setError(null);
      return;
    }

    setLoading(true);
    setError(null);

    fetch(`/api/agent/search?q=${encodeURIComponent(q.trim())}&limit=10`)
      .then(async (r) => {
        if (r.status === 503) {
          setError("Semantic search not configured");
          setResults([]);
          setLoading(false);
          return;
        }
        if (!r.ok) {
          const body = await r.json().catch(() => ({}));
          setError(body.error ?? `Search failed (${r.status})`);
          setResults([]);
          setLoading(false);
          return;
        }
        const data: AgentSearchResponse = await r.json();
        setResults(data.results);
        setTotalSearched(data.total_events_searched);
        setLoading(false);
      })
      .catch(() => {
        setError("Network error");
        setLoading(false);
      });
  }, []);

  // Debounced search
  useEffect(() => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => search(query), DEBOUNCE_MS);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [query, search]);

  return (
    <div className="flex flex-col gap-2 p-3" data-testid="semantic-search">
      {/* Search input */}
      <input
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Search by meaning..."
        className="w-full bg-[#24283b] text-[#c0caf5] text-xs rounded px-3 py-2 border border-[#2f3348] focus:border-[#7aa2f7] focus:outline-none placeholder-[#565f89]"
        data-testid="semantic-search-input"
      />

      {/* Status */}
      {loading && (
        <div className="text-[11px] text-[#565f89]">Searching...</div>
      )}

      {error && (
        <div
          className="text-[11px] text-[#e0af68] bg-[#24283b] rounded px-2 py-1"
          data-testid="semantic-search-error"
        >
          {error}
        </div>
      )}

      {/* Results */}
      {!loading && !error && results.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <div className="text-[10px] text-[#565f89]">
            {results.length} session{results.length !== 1 ? "s" : ""} found
            {totalSearched > 0 && ` (${totalSearched} events searched)`}
          </div>

          {results.map((session) => (
            <button
              key={session.session_id}
              onClick={() => onSelectSession(session.session_id)}
              className="text-left bg-[#24283b] hover:bg-[#2f3348] rounded px-3 py-2 border border-[#2f3348] transition-colors"
              data-testid="semantic-search-result"
            >
              {/* Session header */}
              <div className="flex items-center justify-between mb-1">
                <span className="text-[11px] text-[#c0caf5] font-medium truncate">
                  {session.label
                    ? truncateSnippet(session.label, 60)
                    : session.session_id.slice(0, 12) + "…"}
                </span>
                <span className="text-[10px] text-[#7aa2f7] ml-2 shrink-0">
                  {formatScore(session.relevance_score)}
                </span>
              </div>

              {/* Project + event count */}
              {(session.project_name || session.event_count > 0) && (
                <div className="text-[10px] text-[#565f89] mb-1">
                  {session.project_name && (
                    <span className="mr-2">{session.project_name}</span>
                  )}
                  {session.event_count > 0 && (
                    <span>{session.event_count} events</span>
                  )}
                </div>
              )}

              {/* Matching event snippets */}
              {session.matching_events.map((evt, i) => (
                <div
                  key={evt.event_id ?? i}
                  className="text-[10px] text-[#a9b1d6] mt-0.5 flex gap-1.5"
                >
                  <span className="text-[#565f89] shrink-0 w-14 text-right">
                    {recordTypeLabel(evt.record_type)}
                  </span>
                  <span className="truncate">
                    {truncateSnippet(evt.snippet, 120)}
                  </span>
                </div>
              ))}
            </button>
          ))}
        </div>
      )}

      {/* No results */}
      {!loading && !error && query.trim() && results.length === 0 && (
        <div className="text-[11px] text-[#565f89]" data-testid="semantic-search-empty">
          No results
        </div>
      )}
    </div>
  );
}
