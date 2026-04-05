/** StoryView — live narrative stream of agent sessions.
 *
 * Renders StructuralTurns as sentence cards, streaming in real time
 * as turns complete. The story writes itself.
 *
 * Features wired from lib/story.ts (all pure functions, all tested):
 *   - Category filtering (pure_text, tool_use, thinking, delegation, error)
 *   - Stats bar with verb distribution
 *   - Env growth + scope depth sparklines
 *   - Session sidebar with turn counts
 */

import { useEffect, useRef, useState, useMemo, useCallback } from "react";
import { TurnCard } from "./TurnCard";
import {
  filterSentences,
  categorizeTurn,
  verbDistribution,
  envGrowthSeries,
  scopeDepthProfile,
  type StoryCategory,
} from "@/lib/story";

import type { PatternView } from "@/types/wire-record";

interface StoryViewProps {
  patterns: readonly PatternView[];
  sessionLabels: Readonly<Record<string, { label: string | null }>>;
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

export function StoryView({ patterns, sessionLabels, selectedSession, onSelectSession }: StoryViewProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const feedRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [activeFilters, setActiveFilters] = useState<Set<StoryCategory>>(new Set());

  // All sentence patterns
  const allSentences = useMemo(() => {
    const filtered = patterns.filter(p => p.type === "turn.sentence");
    return selectedSession ? filtered.filter(p => p.session_id === selectedSession) : filtered;
  }, [patterns, selectedSession]);

  // Apply category filter
  const sentences = useMemo(
    () => filterSentences(allSentences, activeFilters),
    [allSentences, activeFilters],
  );

  // Stats
  const verbs = useMemo(() => verbDistribution(sentences), [sentences]);
  const envSeries = useMemo(() => envGrowthSeries(sentences), [sentences]);
  const depthSeries = useMemo(() => scopeDepthProfile(sentences), [sentences]);
  const terminalCount = useMemo(() => sentences.filter(s => (s.metadata as Record<string, unknown>)?.is_terminal === true).length, [sentences]);
  const continuedCount = sentences.length - terminalCount;

  // Category counts for filter badges
  const categoryCounts = useMemo(() => {
    const counts = new Map<StoryCategory, number>();
    for (const s of allSentences) {
      const cat = categorizeTurn(s);
      counts.set(cat, (counts.get(cat) ?? 0) + 1);
    }
    return counts;
  }, [allSentences]);

  // Sessions with stories — derive labels properly
  const sessionsWithStories = useMemo(() => {
    const counts = new Map<string, number>();
    for (const p of patterns) {
      if (p.type === "turn.sentence") {
        counts.set(p.session_id, (counts.get(p.session_id) ?? 0) + 1);
      }
    }
    return Array.from(counts.entries()).map(([sid, count]) => {
      const raw = sessionLabels[sid]?.label;
      const label = raw && raw !== sid ? raw : null;
      const short = sid.startsWith("agent-") ? sid.slice(0, 14) : sid.slice(0, 8);
      return {
        id: sid,
        label: label ? (label.length > 40 ? label.slice(0, 37) + "..." : label) : short,
        count,
        isAgent: sid.startsWith("agent-"),
      };
    }).sort((a, b) => {
      // Main sessions first, then agents. Within each group, by count desc.
      if (a.isAgent !== b.isAgent) return a.isAgent ? 1 : -1;
      return b.count - a.count;
    });
  }, [patterns, sessionLabels]);

  // Auto-scroll
  useEffect(() => {
    if (autoScroll && bottomRef.current) {
      bottomRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [sentences.length, autoScroll]);

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
        setFocusIndex(i => Math.min(i + 1, sentences.length - 1));
      } else if (e.key === "ArrowUp" || e.key === "k") {
        e.preventDefault();
        setFocusIndex(i => Math.max(i - 1, 0));
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [sentences.length]);

  // Scroll focused card into view
  useEffect(() => {
    if (focusIndex >= 0 && feedRef.current) {
      const cards = feedRef.current.querySelectorAll("[data-turn-card]");
      cards[focusIndex]?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }, [focusIndex]);

  // Mobile sidebar toggle
  const [sidebarOpen, setSidebarOpen] = useState(false);

  return (
    <div className="flex flex-1 min-h-0 relative">
      {/* Mobile sidebar toggle */}
      <button
        onClick={() => setSidebarOpen(!sidebarOpen)}
        className="md:hidden fixed bottom-4 left-4 z-50 w-10 h-10 rounded-full bg-[#7aa2f7] text-[#1a1b26] flex items-center justify-center shadow-lg text-lg"
      >
        {sidebarOpen ? "×" : "☰"}
      </button>

      {/* Sidebar — hidden on mobile unless toggled */}
      <div className={`
        ${sidebarOpen ? "fixed inset-0 z-40 w-72" : "hidden"} md:relative md:block md:w-64
        bg-[#1f2335] border-r border-[#2f3348] overflow-y-auto p-2 flex-shrink-0
      `}>
        <div className="text-xs text-[#565f89] uppercase tracking-wide px-2 py-1 mb-1">
          Sessions
        </div>
        <button
          onClick={() => { onSelectSession(null); setSidebarOpen(false); }}
          className={`w-full text-left px-2 py-1.5 rounded text-sm mb-1 ${
            !selectedSession ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#a9b1d6] hover:bg-[#24283b]"
          }`}
        >
          All ({allSentences.length} turns)
        </button>
        {sessionsWithStories.filter(s => !s.isAgent).map(s => (
          <button
            key={s.id}
            onClick={() => { onSelectSession(s.id); setSidebarOpen(false); }}
            className={`w-full text-left px-2 py-1.5 rounded text-sm truncate ${
              selectedSession === s.id ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#a9b1d6] hover:bg-[#24283b]"
            }`}
            title={s.id}
          >
            {s.label} <span className="text-[10px] opacity-60">({s.count})</span>
          </button>
        ))}
        {sessionsWithStories.some(s => s.isAgent) && (
          <>
            <div className="text-[10px] text-[#565f89] uppercase tracking-wide px-2 py-1 mt-3 mb-1">
              Sub-agents
            </div>
            {sessionsWithStories.filter(s => s.isAgent).map(s => (
              <button
                key={s.id}
                onClick={() => { onSelectSession(s.id); setSidebarOpen(false); }}
                className={`w-full text-left px-2 py-1 rounded text-xs truncate ${
                  selectedSession === s.id ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#565f89] hover:bg-[#24283b]"
                }`}
                title={s.id}
              >
                {s.label} <span className="text-[10px] opacity-60">({s.count})</span>
              </button>
            ))}
          </>
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

        {/* Turn cards */}
        <div
          ref={feedRef}
          className="flex-1 overflow-y-auto p-2 sm:p-4 max-w-4xl"
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
            sentences.map((p, i) => (
              <div
                key={`${p.session_id}-${(p.metadata?.turn as number) ?? i}`}
                data-turn-card
                className={focusIndex === i ? "ring-1 ring-[#7aa2f7] rounded-lg" : ""}
              >
                <TurnCard pattern={p} />
              </div>
            ))
          )}
          <div ref={bottomRef} />
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
