/** StoryView — live narrative stream of agent sessions.
 *
 * Renders StructuralTurns as sentence cards, streaming in real time
 * as turns complete. The story writes itself.
 *
 * Data sources:
 *   - Historical: GET /api/sessions/{id}/patterns?type=turn.sentence
 *   - Live: WebSocket state.patterns (accumulated by enrichedReducer)
 */

import { useEffect, useRef, useState, useMemo } from "react";
import { TurnCard } from "./TurnCard";

import type { PatternView } from "@/types/wire-record";

interface StoryViewProps {
  /** All patterns from the WebSocket state (live + initial). */
  patterns: readonly PatternView[];
  /** All session labels for the sidebar. */
  sessionLabels: Readonly<Record<string, { label: string | null }>>;
  /** Currently selected session (null = all sessions). */
  selectedSession: string | null;
  onSelectSession: (sid: string | null) => void;
}

export function StoryView({ patterns, sessionLabels, selectedSession, onSelectSession }: StoryViewProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  // Filter for turn.sentence patterns
  const sentences = useMemo(() => {
    const filtered = patterns.filter(p => p.type === "turn.sentence");
    if (selectedSession) {
      return filtered.filter(p => p.session_id === selectedSession);
    }
    return filtered;
  }, [patterns, selectedSession]);

  // Auto-scroll to bottom when new sentences arrive
  useEffect(() => {
    if (autoScroll && bottomRef.current) {
      bottomRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [sentences.length, autoScroll]);

  // Get unique sessions that have sentences
  const sessionsWithStories = useMemo(() => {
    const sids = new Set(patterns.filter(p => p.type === "turn.sentence").map(p => p.session_id));
    return Array.from(sids).map(sid => ({
      id: sid,
      label: sessionLabels[sid]?.label ?? sid.slice(0, 12),
      count: patterns.filter(p => p.type === "turn.sentence" && p.session_id === sid).length,
    })).sort((a, b) => b.count - a.count);
  }, [patterns, sessionLabels]);

  return (
    <div className="flex flex-1 min-h-0">
      {/* Session sidebar */}
      <div className="w-64 bg-[#1f2335] border-r border-[#2f3348] overflow-y-auto p-2">
        <div className="text-xs text-[#565f89] uppercase tracking-wide px-2 py-1 mb-1">
          Sessions with stories
        </div>
        <button
          onClick={() => onSelectSession(null)}
          className={`w-full text-left px-2 py-1.5 rounded text-sm ${
            !selectedSession ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#a9b1d6] hover:bg-[#24283b]"
          }`}
        >
          All sessions ({sentences.length})
        </button>
        {sessionsWithStories.map(s => (
          <button
            key={s.id}
            onClick={() => onSelectSession(s.id)}
            className={`w-full text-left px-2 py-1.5 rounded text-sm truncate ${
              selectedSession === s.id ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#a9b1d6] hover:bg-[#24283b]"
            }`}
          >
            {s.label} <span className="text-[10px] opacity-60">({s.count})</span>
          </button>
        ))}
      </div>

      {/* Story feed */}
      <div
        className="flex-1 overflow-y-auto p-4 max-w-4xl"
        onScroll={(e) => {
          const el = e.currentTarget;
          const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 100;
          setAutoScroll(atBottom);
        }}
      >
        {sentences.length === 0 ? (
          <div className="text-center text-[#565f89] mt-20">
            <p className="text-lg">No stories yet.</p>
            <p className="text-sm mt-2">Stories appear as agent turns complete.</p>
          </div>
        ) : (
          <>
            <div className="mb-4 text-xs text-[#565f89]">
              {sentences.length} turns · the coalgebra unfolds
            </div>
            {sentences.map((p, i) => (
              <TurnCard key={`${p.session_id}-${(p.metadata?.turn as number) ?? i}`} pattern={p} />
            ))}
          </>
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
