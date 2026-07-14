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
import { useResizablePanel } from "@/hooks/use-resizable-panel";
import { cn } from "@/lib/cn";
import { TurnCard } from "./TurnCard";
import { Clamp } from "@/components/ui/Clamp";
import {
  filterSentences,
  categorizeTurn,
  verbDistribution,
  envGrowthSeries,
  scopeDepthProfile,
  sentenceHeadline,
  findSentenceIndexByEvent,
  findSentenceIndexByTurn,
  type StoryCategory,
} from "@/lib/story";
import {
  fetchSessions,
  fetchSessionSentences,
  mergeSentences,
  type SessionSort,
  type StorySession,
} from "@/lib/story-api";
import { sessionColor } from "@/lib/session-colors";
import { controlActions$ } from "@/streams/control";
import { postInteraction } from "@/lib/interaction";
import { applyFilters, computeFacets, type OverviewFilters } from "@/lib/sessions-overview";
import { sessionTitle } from "@/lib/session-title";
import { SessionSummaryLoader } from "@/components/viz/SessionSummaryLoader";

import type { PatternView } from "@/types/wire-record";

interface StoryViewProps {
  /** Live patterns from WebSocket stream — used for real-time augmentation. */
  livePatterns: readonly PatternView[];
  selectedSession: string | null;
  onSelectSession: (sid: string | null) => void;
  /** Deep-link target: scroll to + highlight the turn whose events include this
   *  id (`#/story/SES/event/ID`). The map principle for the Story view. */
  eventId?: string;
  /** Drill a turn to its SOURCE — open the event in Explore. Closes the Story
   *  dead end (the map principle: every turn navigates to where it came from). */
  onOpenEvent?: (sessionId: string, eventId: string) => void;
}

const CATEGORY_CONFIG: { key: StoryCategory; label: string; color: string }[] = [
  { key: "pure_text", label: "Text", color: "#9ece6a" },
  { key: "tool_use", label: "Tools", color: "#e0af68" },
  { key: "thinking", label: "Think", color: "#bb9af7" },
  { key: "delegation", label: "Agent", color: "#ff9e64" },
  { key: "error", label: "Error", color: "#f7768e" },
];

const DEFAULT_SESSION_LIMIT = 5;

type TimeWindow = "all" | "today" | "7d" | "30d";

const SORT_OPTIONS: { key: SessionSort; label: string }[] = [
  { key: "latest", label: "Latest" },
  { key: "active", label: "Most active" },
  { key: "tokens", label: "Most tokens" },
];

const TIME_OPTIONS: { key: TimeWindow; label: string }[] = [
  { key: "all", label: "All" },
  { key: "today", label: "Today" },
  { key: "7d", label: "7d" },
  { key: "30d", label: "30d" },
];

/** Convert a TimeWindow choice into an RFC 3339 cutoff timestamp, or null
 * for "all" (no filter). Cutoffs are based on the user's local clock. */
function timeWindowToSince(window: TimeWindow): string | undefined {
  if (window === "all") return undefined;
  const now = new Date();
  if (window === "today") {
    const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    return midnight.toISOString();
  }
  const days = window === "7d" ? 7 : 30;
  const cutoff = new Date(now.getTime() - days * 24 * 60 * 60 * 1000);
  return cutoff.toISOString();
}

/** Compact token formatter — `12345` → `12.3k`, `1234567` → `1.2M`. */
function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

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

export function StoryView({ livePatterns, selectedSession, onSelectSession, eventId, onOpenEvent }: StoryViewProps) {
  const feedRef = useRef<HTMLDivElement>(null);
  // Left sidebar with a right-edge grip; width survives reloads.
  const sidebarPanel = useResizablePanel("story.sidebar.width", 300, 200, 480, "left");
  const [autoScroll, setAutoScroll] = useState(true);
  const [activeFilters, setActiveFilters] = useState<Set<StoryCategory>>(new Set());

  // ── REST state ──
  const [sessions, setSessions] = useState<StorySession[]>([]);
  const [sessionsTotal, setSessionsTotal] = useState(0);
  const [sessionsLoading, setSessionsLoading] = useState(true);
  const [sessionLimit, setSessionLimit] = useState(DEFAULT_SESSION_LIMIT);
  const [sortMode, setSortMode] = useState<SessionSort>("latest");
  const [timeWindow, setTimeWindow] = useState<TimeWindow>("all");

  // Agent-in-UI: apply `story.sort` toggle intents (component-local). Sink only.
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type === "toggle" && a.target === "story.sort" && SORT_OPTIONS.some((o) => o.key === a.value)) {
        setSortMode(a.value as SessionSort);
      }
    });
    return () => sub.unsubscribe();
  }, []);
  const [sentenceCache, setSentenceCache] = useState<Map<string, PatternView[]>>(new Map());
  const sentenceCacheRef = useRef(sentenceCache);
  sentenceCacheRef.current = sentenceCache;
  const [loadingSentences, setLoadingSentences] = useState(false);

  // Fetch sessions on mount and when limit/sort/time changes.
  useEffect(() => {
    let cancelled = false;
    setSessionsLoading(true);
    fetchSessions({
      limit: sessionLimit,
      sort: sortMode,
      since: timeWindowToSince(timeWindow),
    })
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
  }, [sessionLimit, sortMode, timeWindow]);

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

  // Deep-link (map principle): when `#/story/SES/event/ID` carries an eventId,
  // scroll the Story to the turn that produced it. Turn off auto-scroll so the
  // deep-link isn't yanked to the bottom by a live append.
  useEffect(() => {
    if (!eventId || sentences.length === 0) return;
    const idx = findSentenceIndexByEvent(sentences, eventId);
    if (idx >= 0) {
      setAutoScroll(false);
      virtualizer.scrollToIndex(idx, { align: "center" });
    }
  }, [eventId, sentences, virtualizer]);

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
  // Sidebar spine: show all sentences past the first 8 (reset per session).
  const [spineExpanded, setSpineExpanded] = useState(false);
  useEffect(() => {
    setSpineExpanded(false);
  }, [selectedSession]);

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
  // Desktop: open. Phones: closed — the feed owns the screen, ☰ opens it.
  const [sidebarOpen, setSidebarOpen] = useState(() => typeof window === "undefined" || window.innerWidth >= 768);

  // Client-side find: free-text search + facet filters over the loaded sessions.
  const [search, setSearch] = useState("");
  const [sidebarFilters, setSidebarFilters] = useState<OverviewFilters>({});

  // Load more sessions
  const handleLoadMore = useCallback(() => {
    setSessionLimit(prev => prev + 10);
  }, []);

  const baseSessions = useMemo(
    () => sessions.filter(s => !s.session_id.startsWith("agent-")),
    [sessions],
  );
  const facets = useMemo(() => computeFacets(baseSessions), [baseSessions]);
  const visibleSessions = useMemo(
    () => applyFilters(baseSessions, { ...sidebarFilters, search }),
    [baseSessions, sidebarFilters, search],
  );
  const findActive = search.trim().length > 0 || Object.keys(sidebarFilters).length > 0;
  const toggleFacet = useCallback((key: keyof Omit<OverviewFilters, "range">, val: string) => {
    setSidebarFilters(f => {
      const next = { ...f };
      if (next[key] === val) delete next[key];
      else next[key] = val;
      return next;
    });
  }, []);

  return (
    <div className="flex flex-1 min-h-0 relative">
      {/* Sidebar open button — visible when sidebar is closed */}
      {!sidebarOpen && (
        <button
          onClick={() => setSidebarOpen(true)}
          className="absolute top-2 left-2 z-50 w-8 h-8 rounded bg-[color:var(--bg-surface)] border border-[color:var(--border)] text-[color:var(--accent)] flex items-center justify-center shadow-lg text-sm hover:bg-[color:var(--bg-hover)] transition-colors"
          title="Open sidebar"
        >
          ☰
        </button>
      )}

      {/* Sidebar */}
      {sidebarOpen && (
      <div
        className="relative bg-[color:var(--bg-surface)] overflow-y-auto flex-shrink-0 flex flex-col"
        style={{ width: sidebarPanel.width }}
      >
        {/* drag handle: right-edge grip, persisted width */}
        <div
          onPointerDown={sidebarPanel.onHandlePointerDown}
          className={`absolute right-0 top-0 z-10 h-full w-1.5 cursor-col-resize transition-colors hover:bg-[color:var(--accent)]/40 ${sidebarPanel.dragging ? "bg-[color:var(--accent)]/60" : "bg-transparent"}`}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize sidebar"
          title="Drag to resize"
        />
        {/* Header bar */}
        <div className="flex items-center justify-between px-3 py-2 border-b border-[color:var(--bg-hover)] bg-[color:var(--bg)] shrink-0">
          <span className="text-[11px] text-[color:var(--text-muted)] uppercase tracking-wider font-semibold">Sessions</span>
          <button
            onClick={() => setSidebarOpen(false)}
            className="w-6 h-6 rounded flex items-center justify-center text-[color:var(--text-muted)] hover:text-[color:var(--text)] hover:bg-[color:var(--bg-surface)] transition-colors text-base"
            title="Close sidebar"
          >
            ×
          </button>
        </div>
        {/* Find bar — free-text search + facet filters over loaded sessions. */}
        <div className="px-3 py-2 border-b border-[color:var(--bg-hover)] bg-[color:var(--bg)] shrink-0 space-y-1.5">
          <div className="relative">
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Find a session…"
              className="w-full rounded border border-[color:var(--border)] bg-[color:var(--bg-surface)] px-2 py-1 pr-6 text-[12px] text-[color:var(--text)] placeholder:text-[color:var(--text-muted)] focus:border-[color:var(--accent)] focus:outline-none"
            />
            {search && (
              <button
                onClick={() => setSearch("")}
                className="absolute right-1.5 top-1/2 -translate-y-1/2 text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
                title="Clear search"
              >
                ×
              </button>
            )}
          </div>
          {(facets.projects.length > 1 || facets.users.length > 1) && (
            <div className="flex flex-wrap gap-1">
              {facets.projects.slice(0, 3).map((p) => (
                <button
                  key={`proj-${p.key}`}
                  onClick={() => toggleFacet("project", p.key)}
                  className={`text-[10px] px-1.5 py-0.5 rounded-full border transition-all ${
                    sidebarFilters.project === p.key
                      ? "border-[color:var(--cyan-bright)] text-[color:var(--cyan-bright)] bg-[color:var(--cyan-bright)]/9"
                      : "border-[color:var(--border)] text-[color:var(--text-muted)] hover:text-[color:var(--text-bright)]"
                  }`}
                  title={`${p.count} sessions`}
                >
                  {p.key.replace(/^-/, "").split(/[-/]/).pop()} · {p.count}
                </button>
              ))}
              {facets.users.slice(0, 3).map((u) => (
                <button
                  key={`user-${u.key}`}
                  onClick={() => toggleFacet("user", u.key)}
                  className={`text-[10px] px-1.5 py-0.5 rounded-full border transition-all ${
                    sidebarFilters.user === u.key
                      ? "border-[color:var(--green)] text-[color:var(--green)] bg-[color:var(--green)]/9"
                      : "border-[color:var(--border)] text-[color:var(--text-muted)] hover:text-[color:var(--text-bright)]"
                  }`}
                  title={`${u.count} sessions`}
                >
                  @{u.key}
                </button>
              ))}
              {findActive && (
                <button
                  onClick={() => { setSearch(""); setSidebarFilters({}); }}
                  className="text-[10px] px-1.5 py-0.5 rounded-full border border-[color:var(--red)]/40 text-[color:var(--red)] hover:bg-[color:var(--red)]/10"
                >
                  clear
                </button>
              )}
            </div>
          )}
        </div>
        {/* Filter strip — sort + time window. Changing either resets paging. */}
        <div className="px-3 py-2 border-b border-[color:var(--bg-hover)] bg-[color:var(--bg)] shrink-0 space-y-1.5">
          <div className="flex flex-wrap gap-1">
            {SORT_OPTIONS.map(opt => {
              const active = sortMode === opt.key;
              return (
                <button
                  key={opt.key}
                  type="button"
                  onClick={() => {
                    if (sortMode !== opt.key) {
                      setSortMode(opt.key);
                      setSessionLimit(DEFAULT_SESSION_LIMIT);
                    }
                  }}
                  className={`text-[10px] px-2 py-0.5 rounded-full border transition-all ${
                    active
                      ? "border-[color:var(--accent)] text-[color:var(--accent)] bg-[color:var(--accent)]/9"
                      : "border-[color:var(--border)] text-[color:var(--text-muted)] hover:text-[color:var(--text-bright)]"
                  }`}
                >
                  {opt.label}
                </button>
              );
            })}
          </div>
          <div className="flex flex-wrap gap-1">
            {TIME_OPTIONS.map(opt => {
              const active = timeWindow === opt.key;
              return (
                <button
                  key={opt.key}
                  type="button"
                  onClick={() => {
                    if (timeWindow !== opt.key) {
                      setTimeWindow(opt.key);
                      setSessionLimit(DEFAULT_SESSION_LIMIT);
                    }
                  }}
                  className={`text-[10px] px-2 py-0.5 rounded-full border transition-all ${
                    active
                      ? "border-[color:var(--purple)] text-[color:var(--purple)] bg-[color:var(--purple)]/9"
                      : "border-[color:var(--border)] text-[color:var(--text-muted)] hover:text-[color:var(--text-bright)]"
                  }`}
                >
                  {opt.label}
                </button>
              );
            })}
          </div>
        </div>
        <div className="flex-1 overflow-y-auto p-2">

        {/* Session loading indicator */}
        {sessionsLoading && sessions.length === 0 && (
          <div className="text-center text-[color:var(--text-muted)] text-sm py-4">Loading sessions...</div>
        )}

        {/* Empty find result */}
        {!sessionsLoading && findActive && visibleSessions.length === 0 && (
          <div className="text-center text-[color:var(--text-muted)] text-xs py-4">No loaded sessions match. Try “Load more”.</div>
        )}

        {/* Session list */}
        {visibleSessions.map(s => {
          const isActive = selectedSession === s.session_id;
          const color = sessionColor(s.session_id);
          const cleaned = sessionTitle(s);
          const label = cleaned.length > 40 ? cleaned.slice(0, 37) + "..." : cleaned;
          const cached = sentenceCache.get(s.session_id);
          const cachedCount = cached?.length;
          const recency = s.last_event ? formatRecency(s.last_event) : null;
          return (
            <div key={s.session_id}>
            <button
              type="button"
              onClick={() => onSelectSession(isActive ? null : s.session_id)}
              className={`w-full text-left px-2 py-2 rounded mb-0.5 transition-colors ${
                isActive
                  ? "bg-[color:var(--bg-hover)] border-l-[3px] border-y border-r border-[color:var(--border)]"
                  : "hover:bg-[color:var(--bg-surface)] border border-transparent"
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
                    <div className={`text-sm truncate ${isActive ? "text-[color:var(--text)] font-medium" : "text-[color:var(--text)]"}`}>
                      {label}
                    </div>
                    {recency && (
                      <span className="text-[10px] text-[color:var(--text-muted)] shrink-0" title={s.last_event ?? undefined}>
                        {recency}
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-1.5 mt-0.5 text-[10px] text-[color:var(--text-muted)] truncate">
                    {s.project_name && (
                      <span className="truncate" title={s.project_name}>
                        {s.project_name}
                      </span>
                    )}
                    {s.project_name && s.event_count != null && <span>·</span>}
                    {s.event_count != null && (
                      <span>{s.event_count} events</span>
                    )}
                    {(() => {
                      const tokens =
                        (s.total_input_tokens ?? 0) +
                        (s.total_output_tokens ?? 0);
                      return tokens > 0 ? (
                        <>
                          <span>·</span>
                          <span title={`${tokens.toLocaleString()} tokens`}>
                            {formatTokens(tokens)} tok
                          </span>
                        </>
                      ) : null;
                    })()}
                    {cachedCount != null && (
                      <>
                        <span>·</span>
                        <span>{cachedCount} turns</span>
                      </>
                    )}
                  </div>
                </div>
              </div>
            </button>
            {/* The story, unfurled: the session's key sentences as a scannable
                narrative spine — what it did, and (muted) why. */}
            {isActive && cached && cached.length > 0 && (
              <div className="ml-3 mt-1 mb-1.5 border-l border-[color:var(--bg-hover)] pl-2.5">
                {(spineExpanded ? cached : cached.slice(0, 8)).map((p, i) => {
                  const h = sentenceHeadline(p);
                  const turn = p.metadata?.turn as number | undefined;
                  return (
                    <button
                      key={i}
                      data-testid={`spine-sentence-${i}`}
                      onClick={() => {
                        // Jump the feed to this sentence's turn card.
                        const idx = findSentenceIndexByTurn(sentences, turn);
                        if (idx >= 0) {
                          // Off auto-scroll first, or a live append yanks the
                          // jump back to the bottom (same as the deep-link path).
                          setAutoScroll(false);
                          setFocusIndex(idx);
                          virtualizer.scrollToIndex(idx, { align: "center" });
                        }
                      }}
                      className="block w-full rounded py-0.5 text-left hover:bg-[color:var(--bg-surface)]"
                    >
                      <div className="text-[11px] leading-snug text-[color:var(--text-bright)]">
                        <span className="text-[color:var(--text-muted)]">{i + 1}.</span> {h.text}
                      </div>
                      {h.because && (
                        <Clamp
                          text={`“${((p.metadata?.human as { content?: string } | undefined)?.content ?? "").trim() || h.because}”`}
                          className="block text-[10px] italic leading-snug text-[color:var(--text-muted)]"
                        />
                      )}
                    </button>
                  );
                })}
                {cached.length > 8 && (
                  <button
                    data-testid="spine-show-all"
                    onClick={() => setSpineExpanded((v) => !v)}
                    className="py-0.5 text-[10px] text-[color:var(--accent)] hover:text-[color:var(--accent)]"
                  >
                    {spineExpanded ? "show fewer ↑" : `+${cached.length - 8} more turns ↓`}
                  </button>
                )}
              </div>
            )}
            </div>
          );
        })}

        {/* Sub-agents */}
        {sessions.some(s => s.session_id.startsWith("agent-")) && (
          <>
            <div className="flex items-center justify-between px-2 py-1 mt-3 mb-1">
              <span className="text-[10px] text-[color:var(--text-muted)] uppercase tracking-wide">Agents</span>
              <span className="text-[10px] text-[color:var(--text-muted)]">{sessions.filter(s => s.session_id.startsWith("agent-")).length}</span>
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
                      ? "bg-[color:var(--bg-hover)] border-l-[3px] border-y border-r border-[color:var(--border)] text-[color:var(--text)]"
                      : "text-[color:var(--text-muted)] hover:bg-[color:var(--bg-surface)] border border-transparent"
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
            className="w-full text-center text-[11px] text-[color:var(--accent)] hover:text-[color:var(--accent)] py-2 mt-2 border border-[color:var(--border)] rounded hover:bg-[color:var(--bg-surface)] transition-colors"
          >
            Load more ({sessionsTotal - sessions.length} remaining)
          </button>
        )}

        {/* Sparklines */}
        {sentences.length > 2 && (
          <div className="mt-4 px-2">
            <div className="text-[10px] text-[color:var(--text-muted)] uppercase tracking-wide mb-1">Env growth</div>
            <Sparkline data={envSeries} color="#7aa2f7" />
            {depthSeries.some(d => d > 0) && (
              <>
                <div className="text-[10px] text-[color:var(--text-muted)] uppercase tracking-wide mb-1 mt-2">Scope depth</div>
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
        {/* Shared summary spine — same header as Explore / Overview.
            When the sidebar is closed the floating ☰ occupies the top-left,
            so give the strip room instead of letting the button cover it. */}
        {selectedSession && (
          <div
            className={cn(
              "bg-[color:var(--bg-surface)] flex-shrink-0",
              !sidebarOpen && "pl-12",
            )}
          >
            <SessionSummaryLoader sessionId={selectedSession} />
          </div>
        )}

        {/* Stats bar + filters */}
        {sentences.length > 0 && (
          <div className="px-4 py-2 bg-[color:var(--bg-surface)] border-b border-[color:var(--bg-hover)] flex-shrink-0">
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-xs text-[color:var(--text-bright)]">
                <b className="text-[color:var(--text)]">{sentences.length}</b> turns
              </span>
              <span className="text-xs text-[color:var(--text-bright)]">
                <b className="text-[color:var(--green)]">{terminalCount}</b> terminated
              </span>
              <span className="text-xs text-[color:var(--text-bright)]">
                <b className="text-[color:var(--orange)]">{continuedCount}</b> continued
              </span>
              {/* Verb distribution */}
              <span className="text-xs text-[color:var(--text-muted)]">·</span>
              {Array.from(verbs.entries())
                .sort((a, b) => b[1] - a[1])
                .slice(0, 5)
                .map(([verb, count]) => (
                  <span key={verb} className="text-xs text-[color:var(--text-bright)]">
                    <b className="text-[color:var(--text)]">{count}</b> {verb}
                  </span>
                ))}
            </div>
            {/* Category filters */}
            <div className="flex flex-wrap gap-1.5 mt-1.5">
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
            <div className="text-center text-[color:var(--text-muted)] mt-20">
              <p className="text-sm">Loading sentences...</p>
            </div>
          ) : sentences.length === 0 ? (
            <div className="text-center text-[color:var(--text-muted)] mt-20">
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
                    className={focusIndex === virtualRow.index ? "ring-1 ring-[color:var(--accent)] rounded-lg" : ""}
                    onClick={() => {
                      // Emit a typed `select` interaction: which turn/eval you
                      // touched, so an agent can read exactly what you clicked.
                      const m = (p.metadata ?? {}) as Record<string, unknown>;
                      const evalObj = m.eval as { content?: string } | undefined;
                      postInteraction({
                        kind: "select",
                        view: "story",
                        session_id: p.session_id,
                        turn: typeof m.turn === "number" ? m.turn : undefined,
                        eventId: p.events[p.events.length - 1],
                        eval: typeof evalObj?.content === "string" ? evalObj.content.slice(0, 500) : undefined,
                      });
                    }}
                  >
                    <TurnCard
                      pattern={p}
                      onSelectSession={onSelectSession}
                      isSelectedSession={selectedSession === p.session_id}
                      onOpenEvent={onOpenEvent ? (eid) => onOpenEvent(p.session_id, eid) : undefined}
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
