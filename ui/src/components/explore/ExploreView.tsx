/** Explore tab — THE sessions browser: sidebar (search / sort / range / facets /
 *  hierarchy list) + detail pane (conversation-forward session view, events,
 *  plans, graph, search) + dashboard landing (stats, calendar, recents) when
 *  no session is selected. Absorbs the former Overview tab; filter state is
 *  URL-owned (route.explore) so every view is bookmarkable.
 */

import { useRef, useCallback, useEffect, useMemo, useState } from "react";
import { ExploreSidebar } from "./ExploreSidebar";
import { ExploreDashboard } from "./ExploreDashboard";
import { ExploreDetail } from "./ExploreDetail";
import { SessionTimeline } from "./SessionTimeline";
import { ConversationView } from "./ConversationView";
import { SemanticSearch } from "./SemanticSearch";
import { PlanViewer } from "@/components/plans/PlanViewer";
import { ConstellationView } from "@/components/viz/ConstellationView";
import { SessionVizLoader } from "@/components/viz/SessionVizLoader";
import { SessionDetailPanel } from "@/components/session/SessionDetailPanel";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { useRecents } from "@/hooks/use-recents";
import { isSubagentSession } from "@/lib/subagents";
import {
  applyFilters,
  computeStats,
  hasActiveFilters,
  pickRecentSessions,
  type OverviewFilters,
  type SortKey,
} from "@/lib/sessions-overview";
import { buildHash, type ExploreQuery, type HashRoute } from "@/lib/hash-route";

export type DetailView = "session" | "events" | "conversation" | "plans" | "graph" | "search";

/** The default detail view — conversation-forward (summary + tokens + transcript),
 *  not the busy Events/Tool-Journey/Files wall. */
export const DEFAULT_DETAIL_VIEW: DetailView = "session";

export const VIEW_TABS: { key: DetailView; label: string }[] = [
  { key: "session", label: "Session" },
  { key: "events", label: "Events" },
  { key: "plans", label: "Plans" },
  { key: "graph", label: "Graph" },
  { key: "search", label: "Search" },
];

interface ExploreViewProps {
  route: HashRoute;
  onNavigate: (route: HashRoute) => void;
}

export function ExploreView({ route, onNavigate }: ExploreViewProps) {
  const selectedSessionId = route.sessionId ?? null;
  const detailView: DetailView = route.detailView ?? DEFAULT_DETAIL_VIEW;
  const cameFromSearch = useRef(false);

  const { sessions, loading, error, refresh } = useSessionsList();
  const { recentIds, record } = useRecents();

  // Phones get a drawer, not a sliver: below md the sidebar starts closed
  // behind a ☰ toggle and closes itself after a selection.
  const isNarrow = () => typeof window !== "undefined" && window.innerWidth < 768;
  const [sidebarOpen, setSidebarOpen] = useState(() => !isNarrow());

  // ── URL-owned filter state ────────────────────────────────────────────────
  // Hydrated from the route; local state is the interactive truth, mirrored
  // back into the URL below (replaceState — no history spam per keystroke).
  const [filters, setFilters] = useState<OverviewFilters>(() => route.explore?.filters ?? {});
  const [sortKey, setSortKey] = useState<SortKey>(() => route.explore?.sort ?? "recent");

  // Adopt externally-navigated filter state (pasted link, back button, agent
  // drive). Our own replaceState mirror never fires hashchange, so this only
  // runs on real navigations.
  const externalQuery = route.explore;
  useEffect(() => {
    const local: ExploreQuery = { filters, ...(sortKey !== "recent" ? { sort: sortKey } : {}) };
    const incoming: ExploreQuery = externalQuery ?? { filters: {} };
    if (JSON.stringify(incoming) !== JSON.stringify(local)) {
      setFilters(incoming.filters);
      setSortKey(incoming.sort ?? "recent");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [externalQuery]);

  // The query tail every navigation carries so filters survive path changes.
  const exploreQuery: ExploreQuery | undefined = useMemo(() => {
    const hasSort = sortKey !== "recent";
    if (!hasActiveFilters(filters) && !hasSort) return undefined;
    return { filters, ...(hasSort ? { sort: sortKey } : {}) };
  }, [filters, sortKey]);

  // Mirror filter state → address bar (canonical, copyable).
  useEffect(() => {
    if (detailView === "search") return; // #/search?q= owns that URL shape
    const hash = buildHash({
      view: "explore",
      sessionId: route.sessionId,
      detailView: route.detailView,
      eventId: route.eventId,
      filePath: route.filePath,
      ...(exploreQuery ? { explore: exploreQuery } : {}),
    });
    if (hash !== window.location.hash) {
      window.history.replaceState(null, "", hash);
    }
  }, [exploreQuery, route.sessionId, route.detailView, route.eventId, route.filePath, detailView]);

  // ── dashboard data (over the parent universe) ─────────────────────────────
  const universe = useMemo(
    () => sessions.filter((s) => !isSubagentSession(s.session_id)),
    [sessions],
  );
  const filteredUniverse = useMemo(() => applyFilters(universe, filters), [universe, filters]);
  const stats = useMemo(() => computeStats(filteredUniverse), [filteredUniverse]);
  const recentSessions = useMemo(
    () => pickRecentSessions(universe, recentIds, 5),
    [universe, recentIds],
  );

  const handleSelectSession = useCallback((id: string) => {
    record(id);
    if (isNarrow()) setSidebarOpen(false);
    onNavigate({ view: "explore", sessionId: id, explore: exploreQuery });
  }, [onNavigate, exploreQuery, record]);

  const handleSearchSelect = useCallback((id: string) => {
    cameFromSearch.current = true;
    onNavigate({ view: "explore", sessionId: id, detailView: "session", explore: exploreQuery });
  }, [onNavigate, exploreQuery]);

  const handleBackToSearch = useCallback(() => {
    cameFromSearch.current = false;
    onNavigate({ view: "explore", detailView: "search", searchQuery: route.searchQuery });
  }, [onNavigate, route.searchQuery]);

  const handleDetailTab = useCallback((key: DetailView) => {
    onNavigate({ ...route, detailView: key, eventId: undefined, filePath: undefined, explore: exploreQuery });
  }, [onNavigate, route, exploreQuery]);

  // View tab bar — shared between session and no-session states
  const tabBar = (
    <div className="flex items-center gap-1 px-4 py-2 border-t border-[#2f3348]">
      {/* Back to search button — shown when navigated from a search result */}
      {cameFromSearch.current && detailView !== "search" && (
        <button
          onClick={handleBackToSearch}
          className="px-2 py-1 rounded text-xs text-[#7aa2f7] hover:bg-[#24283b] mr-1"
          data-testid="back-to-search"
        >
          &larr; Search
        </button>
      )}
      {VIEW_TABS.map(({ key, label }) => (
        <button
          key={key}
          onClick={() => handleDetailTab(key)}
          data-testid={`view-toggle-${key}`}
          className={`px-3 py-1 rounded text-xs transition-colors ${
            detailView === key
              ? "bg-[#7aa2f7] text-[#1a1b26] font-medium"
              : "text-[#565f89] hover:text-[#c0caf5] hover:bg-[#24283b]"
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  );

  return (
    <div className="relative flex flex-1 min-h-0" data-testid="explore-view">
      {/* Mobile: the sessions drawer toggle (same idiom as Story's ☰) */}
      {!sidebarOpen && (
        <button
          onClick={() => setSidebarOpen(true)}
          data-testid="sidebar-toggle"
          className="absolute bottom-3 left-3 z-40 flex h-9 w-9 items-center justify-center rounded-full border border-[#3b4261] bg-[#24283b] text-sm text-[#7aa2f7] shadow-lg md:hidden"
          title="Sessions"
        >
          ☰
        </button>
      )}
      {/* Scrim behind the mobile drawer */}
      {sidebarOpen && (
        <div
          className="fixed inset-0 z-20 bg-black/50 md:hidden"
          onClick={() => setSidebarOpen(false)}
          aria-hidden
        />
      )}
      {/* md:contents dissolves this wrapper on desktop — the sidebar stays a
          normal flex child there; below md it's an off-canvas drawer. */}
      <div
        data-testid="explore-drawer"
        data-state={sidebarOpen ? "open" : "closed"}
        className={
          "max-md:absolute max-md:inset-y-0 max-md:left-0 max-md:z-30 max-md:w-72 max-md:shadow-2xl max-md:transition-transform md:contents" +
          (sidebarOpen ? "" : " max-md:-translate-x-full")
        }
      >
        <ExploreSidebar
          sessions={sessions}
          loading={loading}
          filters={filters}
          sortKey={sortKey}
          onFiltersChange={setFilters}
          onSortChange={setSortKey}
          selectedSessionId={selectedSessionId}
          onSelectSession={handleSelectSession}
        />
      </div>
      <div className="flex-1 min-w-0 overflow-y-auto flex flex-col">
        {selectedSessionId && (
          <div style={{ display: detailView === "search" ? "none" : undefined }}>
            {tabBar}
            {/* Default: conversation-forward — summary card + tokens on top +
                transcript & writes. The busy Events/Tool-Journey/Files wall is
                demoted behind the Events tab. */}
            {detailView === "session" && (
              <>
                <SessionVizLoader
                  sessionId={selectedSessionId}
                  onOpenStory={() => onNavigate({ view: "story", sessionId: selectedSessionId })}
                  onOpenSubagent={(id) => handleSelectSession(id)}
                />
                {/* Synopsis / file-impact / error drills — the former Overview
                    drill-in's deep links stay reachable here. */}
                <SessionDetailPanel sessionId={selectedSessionId} />
              </>
            )}
            {detailView === "events" && (
              <>
                <ExploreDetail sessionId={selectedSessionId} />
                <SessionTimeline
                  sessionId={selectedSessionId}
                  scrollToEventId={route.eventId}
                  initialFilePath={route.filePath}
                />
              </>
            )}
            {detailView === "conversation" && (
              <ConversationView sessionId={selectedSessionId} />
            )}
            {detailView === "plans" && (
              <PlanViewer sessionId={selectedSessionId} />
            )}
            {detailView === "graph" && (
              <ConstellationView
                rootId={selectedSessionId}
                onOpen={(id) => onNavigate({ view: "explore", sessionId: id, detailView: "graph", explore: exploreQuery })}
              />
            )}
          </div>
        )}

        {/* Search — always mounted, hidden when not active. Preserves query/results state. */}
        <div style={{ display: detailView === "search" ? undefined : "none" }}>
          {!selectedSessionId && tabBar}
          <SemanticSearch
            onSelectSession={handleSearchSelect}
            initialQuery={route.searchQuery}
          />
        </div>

        {/* Dashboard landing — no session selected, not on search tab */}
        {!selectedSessionId && detailView !== "search" && (
          <>
            {tabBar}
            <ExploreDashboard
              universe={universe}
              stats={stats}
              filtersActive={hasActiveFilters(filters)}
              selectedDay={filters.day ?? null}
              loading={loading}
              error={error}
              refresh={refresh}
              recentSessions={recentSessions}
              sortKey={sortKey}
              onSortKey={setSortKey}
              onSelectDay={(day) => setFilters((f) => ({ ...f, day: day ?? undefined, range: undefined }))}
              onOpenSession={handleSelectSession}
              onClearFilters={() => setFilters({})}
            />
          </>
        )}
      </div>
    </div>
  );
}
