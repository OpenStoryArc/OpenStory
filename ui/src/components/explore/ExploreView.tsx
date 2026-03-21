/** Explore tab — REST-backed session browser with detail panel, event timeline, and conversation view. */

import { useRef, useCallback } from "react";
import { ExploreSidebar } from "./ExploreSidebar";
import { ExploreDetail } from "./ExploreDetail";
import { SessionTimeline } from "./SessionTimeline";
import { ConversationView } from "./ConversationView";
import { SemanticSearch } from "./SemanticSearch";
import { PlanViewer } from "@/components/plans/PlanViewer";
import type { HashRoute } from "@/lib/hash-route";

export type DetailView = "events" | "conversation" | "plans" | "search";

export const VIEW_TABS: { key: DetailView; label: string }[] = [
  { key: "events", label: "Events" },
  { key: "conversation", label: "Conversation" },
  { key: "plans", label: "Plans" },
  { key: "search", label: "Search" },
];

interface ExploreViewProps {
  route: HashRoute;
  onNavigate: (route: HashRoute) => void;
}

export function ExploreView({ route, onNavigate }: ExploreViewProps) {
  const selectedSessionId = route.sessionId ?? null;
  const detailView: DetailView = route.detailView ?? "events";
  const cameFromSearch = useRef(false);

  const handleSelectSession = useCallback((id: string) => {
    onNavigate({ view: "explore", sessionId: id });
  }, [onNavigate]);

  const handleSearchSelect = useCallback((id: string) => {
    cameFromSearch.current = true;
    onNavigate({ view: "explore", sessionId: id, detailView: "events" });
  }, [onNavigate]);

  const handleBackToSearch = useCallback(() => {
    cameFromSearch.current = false;
    onNavigate({ view: "explore", detailView: "search", searchQuery: route.searchQuery });
  }, [onNavigate, route.searchQuery]);

  const handleDetailTab = useCallback((key: DetailView) => {
    onNavigate({ ...route, detailView: key, eventId: undefined, filePath: undefined });
  }, [onNavigate, route]);

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
    <div className="flex flex-1 min-h-0" data-testid="explore-view">
      <ExploreSidebar
        selectedSessionId={selectedSessionId}
        onSelectSession={handleSelectSession}
      />
      <div className="flex-1 min-w-0 overflow-y-auto">
        {selectedSessionId && (
          <div style={{ display: detailView === "search" ? "none" : undefined }}>
            <ExploreDetail sessionId={selectedSessionId} />
            {tabBar}
            {detailView === "events" && (
              <SessionTimeline
                sessionId={selectedSessionId}
                scrollToEventId={route.eventId}
                initialFilePath={route.filePath}
              />
            )}
            {detailView === "conversation" && (
              <ConversationView sessionId={selectedSessionId} />
            )}
            {detailView === "plans" && (
              <PlanViewer sessionId={selectedSessionId} />
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

        {/* Empty state — no session selected, not on search tab */}
        {!selectedSessionId && detailView !== "search" && (
          <div className="flex items-center justify-center h-full text-[#565f89]">
            <div className="text-center">
              <div className="text-lg mb-2">Select a session</div>
              <div className="text-xs">Choose a session from the sidebar, or use the <button onClick={() => handleDetailTab("search")} className="text-[#7aa2f7] hover:underline">Search</button> tab to find sessions by meaning</div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
