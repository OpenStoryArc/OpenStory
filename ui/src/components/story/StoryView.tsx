/** StoryView — REST-first narrative view of agent sessions.
 *
 * Data flow:
 *   1. On mount: GET /api/sessions?limit=5 → sidebar
 *   2. On session click: GET /api/sessions/{id}/patterns?type=turn.sentence → cards
 *   3. Real-time: WebSocket patterns append to active session via mergeSentences()
 *   4. "Load more" fetches additional sessions
 *
 * Features wired from lib/story.ts (all pure functions, all tested):
 *   - Category filtering (pure_text, tool_use, thinking, delegation, error)
 *   - Stats bar with verb distribution
 *   - Env growth + scope depth sparklines
 *   - Session sidebar with turn counts
 */

import { useEffect, useRef, useState, useMemo, useCallback } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { TurnCard } from "./TurnCard";
import {
  filterSentences,
  categorizeTurn,
  verbDistribution,
  envGrowthSeries,
  scopeDepthProfile,
  type StoryCategory,
} from "@/lib/story";
import {
  fetchSessions,
  fetchSessionSentences,
  mergeSentences,
  type StorySession,
} from "@/lib/story-api";
import { sessionColor } from "@/lib/session-colors";

import type { PatternView } from "@/types/wire-record";

interface StoryViewProps {
  /** Live patterns from WebSocket stream — used for real-time augmentation. */
  livePatterns: readonly PatternView[];
  selectedSession: string | null;
  onSelectSession: (sid: string | null) => void;
}

const CATEGORY_CONFIG: { key: StoryCategory; label: string; color: string }[] = [
  { key: "pure_text", label: "Text", color: "#9ece6a" },
  { key: "tool_use", label: "Tools", color: "#e0af68" },
  { key: "thinking", label: "Think", color: "#bb9af7" },
  { key: "delegation", label: "Agent", color: "#ff9e64" },
  { key: "error", label: "Error", color: "#f7768e" },
];

const DEFAULT_SESSION_LIMIT = 5;

/** Format an ISO timestamp as a human-readable relative time. */
function formatRecency(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  if (isNaN(ms) || ms < 0) return "";
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return "just now";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days === 1) return "yesterday";
  if (days < 7) return `${days}d ago`;
  return new Date(iso).toLocaleDateString();
}

export function StoryView({ livePatterns, selectedSession, onSelectSession }: StoryViewProps) {
  const feedRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [activeFilters, setActiveFilters] = useState<Set<StoryCategory>>(new Set());

  // ── REST state ──
  const [sessions, setSessions] = useState<StorySession[]>([]);
  const [sessionsTotal, setSessionsTotal] = useState(0);
  const [sessionsLoading, setSessionsLoading] = useState(true);
  const [sessionLimit, setSessionLimit] = useState(DEFAULT_SESSION_LIMIT);
  const [sentenceCache, setSentenceCache] = useState<Map<string, PatternView[]>>(new Map());
  const sentenceCacheRef = useRef(sentenceCache);
  sentenceCacheRef.current = sentenceCache;
  const [loadingSentences, setLoadingSentences] = useState(false);

  // Fetch sessions on mount and when limit changes
  useEffect(() => {
    let cancelled = false;
    setSessionsLoading(true);
    fetchSessions(sessionLimit)
      .then(({ sessions: s, total }) => {
        if (!cancelled) {
          setSessions(s);
          setSessionsTotal(total);
          setSessionsLoading(false);
        }
      })
      .catch(() => {
        if (!cancelled) setSessionsLoading(false);
      });
    return () => { cancelled = true; };
  }, [sessionLimit]);

  // Auto-select most recent session if none selected and sessions loaded
  useEffect(() => {
    if (!selectedSession && sessions.length > 0 && !sessionsLoading) {
      onSelectSession(sessions[0]!.session_id);
    }
  }, [sessions, selectedSession, sessionsLoading, onSelectSession]);

  // Fetch sentences when session is selected
  useEffect(() => {
    if (!selectedSession) return;
    if (sentenceCacheRef.current.has(selectedSession)) return; // already cached

    let cancelled = false;
    setLoadingSentences(true);
    fetchSessionSentences(selectedSession)
      .then((patterns) => {
        if (!cancelled) {
          setSentenceCache(prev => new Map(prev).set(selectedSession, patterns));
          setLoadingSentences(false);
        }
      })
      .catch(() => {
        if (!cancelled) setLoadingSentences(false);
      });
    return () => { cancelled = true; };
  }, [selectedSession]);

  // Real-time: merge WebSocket patterns into active session cache
  useEffect(() => {
    if (!selectedSession) return;
    const existing = sentenceCacheRef.current.get(selectedSession);
    if (!existing) return; // haven't loaded from REST yet

    const sessionPatterns = livePatterns.filter(p => p.session_id === selectedSession);
    const merged = mergeSentences(existing, sessionPatterns);
    if (merged) {
      setSentenceCache(prev => new Map(prev).set(selectedSession, merged));
    }
  }, [livePatterns, selectedSession]);

  // Get sentences for the current view
  const currentSentences = selectedSession
    ? (sentenceCache.get(selectedSession) ?? [])
    : [];

  // Apply category filter, then sort by turn number
  const sentences = useMemo(() => {
    const filtered = filterSentences(currentSentences, activeFilters);
    return filtered.sort((a, b) => {
      const ta = (a.metadata?.turn as number) ?? 0;
      const tb = (b.metadata?.turn as number) ?? 0;
      return ta - tb;
    });
  }, [currentSentences, activeFilters]);

  // Stats
  const verbs = useMemo(() => verbDistribution(sentences), [sentences]);
  const envSeries = useMemo(() => envGrowthSeries(sentences), [sentences]);
  const depthSeries = useMemo(() => scopeDepthProfile(sentences), [sentences]);
  const terminalCount = useMemo(() => sentences.filter(s => (s.metadata as Record<string, unknown>)?.is_terminal === true).length, [sentences]);
  const continuedCount = sentences.length - terminalCount;

  // Category counts for filter badges
  const categoryCounts = useMemo(() => {
    const counts = new Map<StoryCategory, number>();
    for (const s of currentSentences) {
      const cat = categorizeTurn(s);
      counts.set(cat, (counts.get(cat) ?? 0) + 1);
    }
    return counts;
  }, [currentSentences]);

  // Virtualizer
  const virtualizer = useVirtualizer({
    count: sentences.length,
    getScrollElement: () => feedRef.current,
    estimateSize: () => 140, // collapsed TurnCard height estimate
    overscan: 5,
    getItemKey: useCallback(
      (index: number) => {
        const s = sentences[index];
        return s ? `${s.session_id}-${(s.metadata?.turn as number) ?? index}` : index;
      },
      [sentences],
    ),
  });

  // Auto-scroll to bottom when new sentences arrive
  const prevCountRef = useRef(0);
  useEffect(() => {
    if (autoScroll && sentences.length > prevCountRef.current && sentences.length > 0) {
      virtualizer.scrollToIndex(sentences.length - 1, { align: "end" });
    }
    prevCountRef.current = sentences.length;
  }, [sentences.length, autoScroll, virtualizer]);

  // Toggle filter
  const toggleFilter = useCallback((cat: StoryCategory) => {
    setActiveFilters(prev => {
      const next = new Set(prev);
      if (next.has(cat)) {
        next.delete(cat);
      } else {
        next.add(cat);
      }
      return next;
    });
  }, []);

  // Keyboard nav
  const [focusIndex, setFocusIndex] = useState(-1);
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown" || e.key === "j") {
        e.preventDefault();
        setFocusIndex(i => {
          const next = Math.min(i + 1, sentences.length - 1);
          virtualizer.scrollToIndex(next, { align: "center" });
          return next;
        });
      } else if (e.key === "ArrowUp" || e.key === "k") {
        e.preventDefault();
        setFocusIndex(i => {
          const next = Math.max(i - 1, 0);
          virtualizer.scrollToIndex(next, { align: "center" });
          return next;
        });
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [sentences.length, virtualizer]);

  // Sidebar toggle
  const [sidebarOpen, setSidebarOpen] = useState(true);

  // Load more sessions
  const handleLoadMore = useCallback(() => {
    setSessionLimit(prev => prev + 10);
  }, []);

  return (
    <div className="flex flex-1 min-h-0 relative">
      {/* Sidebar open button — visible when sidebar is closed */}
      {!sidebarOpen && (
        <button
          onClick={() => setSidebarOpen(true)}
          className="absolute top-2 left-2 z-50 w-8 h-8 rounded bg-[#24283b] border border-[#3b4261] text-[#7aa2f7] flex items-center justify-center shadow-lg text-sm hover:bg-[#2a3050] transition-colors"
          title="Open sidebar"
        >
          ☰
        </button>
      )}

      {/* Sidebar */}
      {sidebarOpen && (
      <div className="relative w-72 md:w-80 bg-[#1f2335] border-r border-[#2f3348] overflow-y-auto flex-shrink-0 flex flex-col">
        {/* Header bar */}
        <div className="flex items-center justify-between px-3 py-2 border-b border-[#2f3348] bg-[#1a1b26] shrink-0">
          <span className="text-[11px] text-[#565f89] uppercase tracking-wider font-semibold">Sessions</span>
          <button
            onClick={() => setSidebarOpen(false)}
            className="w-6 h-6 rounded flex items-center justify-center text-[#565f89] hover:text-[#c0caf5] hover:bg-[#24283b] transition-colors text-base"
            title="Close sidebar"
          >
            ×
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-2">

        {/* Session loading indicator */}
        {sessionsLoading && sessions.length === 0 && (
          <div className="text-center text-[#565f89] text-sm py-4">Loading sessions...</div>
        )}

        {/* Session list */}
        {sessions.filter(s => !s.session_id.startsWith("agent-")).map(s => {
          const isActive = selectedSession === s.session_id;
          const color = sessionColor(s.session_id);
          const label = s.label && s.label !== s.session_id
            ? (s.label.length > 40 ? s.label.slice(0, 37) + "..." : s.label)
            : s.session_id;
          const cachedCount = sentenceCache.get(s.session_id)?.length;
          const recency = s.last_event ? formatRecency(s.last_event) : null;
          return (
            <button
              key={s.session_id}
              type="button"
              onClick={() => onSelectSession(isActive ? null : s.session_id)}
              className={`w-full text-left px-2 py-2 rounded mb-0.5 transition-colors ${
                isActive
                  ? "bg-[#283549] border-l-[3px] border-y border-r border-[#3b4261]"
                  : "hover:bg-[#24283b] border border-transparent"
              }`}
              style={isActive ? { borderLeftColor: color } : undefined}
              title={s.session_id}
            >
              <div className="flex items-start gap-1.5">
                {isActive && (
                  <span
                    className="text-[10px] mt-0.5 shrink-0"
                    style={{ color }}
                  >
                    ●
                  </span>
                )}
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between gap-1">
                    <div className={`text-sm truncate ${isActive ? "text-[#c0caf5] font-medium" : "text-[#c0caf5]"}`}>
                      {label}
                    </div>
                    {recency && (
                      <span className="text-[10px] text-[#565f89] shrink-0" title={s.last_event ?? undefined}>
                        {recency}
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2 mt-0.5">
                    {cachedCount != null && (
                      <span className="text-[10px] text-[#565f89]">
                        {cachedCount} turns
                      </span>
                    )}
                    {s.event_count != null && (
                      <span className="text-[10px] text-[#565f89]">
                        {s.event_count} events
                      </span>
                    )}
                  </div>
                </div>
              </div>
            </button>
          );
        })}

        {/* Sub-agents */}
        {sessions.some(s => s.session_id.startsWith("agent-")) && (
          <>
            <div className="flex items-center justify-between px-2 py-1 mt-3 mb-1">
              <span className="text-[10px] text-[#565f89] uppercase tracking-wide">Agents</span>
              <span className="text-[10px] text-[#565f89]">{sessions.filter(s => s.session_id.startsWith("agent-")).length}</span>
            </div>
            {sessions.filter(s => s.session_id.startsWith("agent-")).map(s => {
              const isActive = selectedSession === s.session_id;
              const color = sessionColor(s.session_id);
              return (
                <button
                  key={s.session_id}
                  type="button"
                  onClick={() => onSelectSession(isActive ? null : s.session_id)}
                  className={`w-full text-left px-2 py-1.5 rounded text-xs truncate mb-0.5 transition-colors ${
                    isActive
                      ? "bg-[#283549] border-l-[3px] border-y border-r border-[#3b4261] text-[#c0caf5]"
                      : "text-[#565f89] hover:bg-[#24283b] border border-transparent"
                  }`}
                  style={isActive ? { borderLeftColor: color } : undefined}
                  title={s.session_id}
                >
                  {isActive && <span className="mr-1" style={{ color }}>●</span>}
                  {s.session_id} <span className="opacity-60">({s.event_count ?? 0})</span>
                </button>
              );
            })}
          </>
        )}

        {/* Load more */}
        {sessions.length < sessionsTotal && (
          <button
            onClick={handleLoadMore}
            className="w-full text-center text-[11px] text-[#7aa2f7] hover:text-[#89b4fa] py-2 mt-2 border border-[#3b4261] rounded hover:bg-[#24283b] transition-colors"
          >
            Load more ({sessionsTotal - sessions.length} remaining)
          </button>
        )}

        {/* Sparklines */}
        {sentences.length > 2 && (
          <div className="mt-4 px-2">
            <div className="text-[10px] text-[#565f89] uppercase tracking-wide mb-1">Env growth</div>
            <Sparkline data={envSeries} color="#7aa2f7" />
            {depthSeries.some(d => d > 0) && (
              <>
                <div className="text-[10px] text-[#565f89] uppercase tracking-wide mb-1 mt-2">Scope depth</div>
                <Sparkline data={depthSeries} color="#bb9af7" />
              </>
            )}
          </div>
        )}
      </div>
      </div>
      )}

      {/* Main feed */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Stats bar + filters */}
        {sentences.length > 0 && (
          <div className="px-4 py-2 bg-[#24283b] border-b border-[#2f3348] flex-shrink-0">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-xs text-[#a9b1d6]">
                <b className="text-[#c0caf5]">{sentences.length}</b> turns
              </span>
              <span className="text-xs text-[#a9b1d6]">
                <b className="text-[#9ece6a]">{terminalCount}</b> terminated
              </span>
              <span className="text-xs text-[#a9b1d6]">
                <b className="text-[#e0af68]">{continuedCount}</b> continued
              </span>
              {/* Verb distribution */}
              <span className="text-xs text-[#565f89]">·</span>
              {Array.from(verbs.entries())
                .sort((a, b) => b[1] - a[1])
                .slice(0, 5)
                .map(([verb, count]) => (
                  <span key={verb} className="text-xs text-[#a9b1d6]">
                    <b className="text-[#c0caf5]">{count}</b> {verb}
                  </span>
                ))}
            </div>
            {/* Category filters */}
            <div className="flex gap-1.5 mt-1.5">
              {CATEGORY_CONFIG.map(({ key, label, color }) => {
                const count = categoryCounts.get(key) ?? 0;
                if (count === 0) return null;
                const active = activeFilters.size === 0 || activeFilters.has(key);
                return (
                  <button
                    key={key}
                    onClick={() => toggleFilter(key)}
                    className={`text-[10px] px-2 py-0.5 rounded-full border transition-all ${
                      active ? "opacity-100" : "opacity-30"
                    }`}
                    style={{
                      borderColor: color,
                      color: active ? color : "#565f89",
                      backgroundColor: active ? `${color}18` : "transparent",
                    }}
                  >
                    {label} ({count})
                  </button>
                );
              })}
            </div>
          </div>
        )}

        {/* Turn cards (virtualized) */}
        <div
          ref={feedRef}
          className="flex-1 overflow-y-auto p-2 sm:p-4 max-w-4xl"
          onScroll={() => {
            const el = feedRef.current;
            if (!el) return;
            const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 100;
            setAutoScroll(atBottom);
          }}
        >
          {loadingSentences ? (
            <div className="text-center text-[#565f89] mt-20">
              <p className="text-sm">Loading sentences...</p>
            </div>
          ) : sentences.length === 0 ? (
            <div className="text-center text-[#565f89] mt-20">
              <p className="text-lg">
                {selectedSession ? "No sentences for this session." : "Select a session to view its story."}
              </p>
              <p className="text-sm mt-2">Sentences appear as agent turns complete.</p>
            </div>
          ) : (
            <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
              {virtualizer.getVirtualItems().map((virtualRow) => {
                const p = sentences[virtualRow.index]!;
                return (
                  <div
                    key={virtualRow.key}
                    style={{
                      position: "absolute",
                      top: 0,
                      left: 0,
                      width: "100%",
                      transform: `translateY(${virtualRow.start}px)`,
                    }}
                    ref={virtualizer.measureElement}
                    data-index={virtualRow.index}
                    data-turn-card
                    className={focusIndex === virtualRow.index ? "ring-1 ring-[#7aa2f7] rounded-lg" : ""}
                  >
                    <TurnCard
                      pattern={p}
                      onSelectSession={onSelectSession}
                      isSelectedSession={selectedSession === p.session_id}
                    />
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────
// Sparkline — minimal inline chart
// ─────────────────────────────────────────────

function Sparkline({ data, color }: { data: number[]; color: string }) {
  if (data.length < 2) return null;
  const max = Math.max(...data, 1);
  const w = 200;
  const h = 24;
  const points = data.map((v, i) => {
    const x = (i / (data.length - 1)) * w;
    const y = h - (v / max) * h;
    return `${x},${y}`;
  }).join(" ");

  return (
    <svg width={w} height={h} className="w-full">
      <polyline
        points={points}
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity="0.6"
      />
    </svg>
  );
}
