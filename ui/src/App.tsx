import { useEffect, useMemo, useState, useCallback } from "react";
import { connect, wsMessages$ } from "@/streams/connection";
import { buildSessionState$ } from "@/streams/sessions";
import { useConnectionStatus } from "@/hooks/use-connection-status";
import { useObservable } from "@/hooks/use-observable";
import { useHashRoute } from "@/hooks/use-hash-route";
import { Timeline } from "@/components/Timeline";
import { Sidebar } from "@/components/Sidebar";
import { TabBar } from "@/components/layout/TabBar";
import { ExploreView } from "@/components/explore/ExploreView";
import { StoryView } from "@/components/story/StoryView";
import { UsersView } from "@/components/users/UsersView";
import { SessionHeader, useSessionHeaderInfo } from "@/components/SessionHeader";
import { useLocalInfo } from "@/hooks/use-local-info";
import { EMPTY_ENRICHED_STATE } from "@/streams/sessions";
import type { ViewMode, CrossLink } from "@/lib/navigation";

const STATUS_INDICATOR = {
  connected: { color: "bg-green-400", label: "Connected" },
  connecting: { color: "bg-yellow-400 animate-pulse", label: "Connecting" },
  disconnected: { color: "bg-red-400", label: "Disconnected" },
} as const;

export function App() {
  useEffect(() => {
    const cleanup = connect();
    return cleanup;
  }, []);

  const state$ = useMemo(() => buildSessionState$(wsMessages$()), []);
  const state = useObservable(state$, EMPTY_ENRICHED_STATE);
  const status = useConnectionStatus();
  const { color, label } = STATUS_INDICATOR[status];

  const [route, navigate] = useHashRoute();
  const [focusAgentId, setFocusAgentId] = useState<string | null>(null);
  const localInfo = useLocalInfo();

  // Derive view state from route
  const viewMode = route.view;
  const selectedSession = route.view === "live" ? (route.sessionId ?? null) : null;
  const storySession = route.view === "story" ? (route.sessionId ?? null) : null;
  // Live tab user filter is owned by the URL — bookmarkable & shareable.
  const userFilter = route.view === "live" ? (route.userFilter ?? null) : null;

  const handleSelectSession = useCallback((sid: string | null) => {
    setFocusAgentId(null);
    // Preserve the active user filter when picking a session — clicking
    // a session inside @max's filtered view shouldn't clear the filter.
    navigate({
      view: "live",
      ...(sid ? { sessionId: sid } : {}),
      ...(userFilter ? { userFilter } : {}),
    });
  }, [navigate, userFilter]);

  const handleUserFilterChange = useCallback((user: string | null) => {
    navigate({
      view: "live",
      ...(selectedSession ? { sessionId: selectedSession } : {}),
      ...(user ? { userFilter: user } : {}),
    });
  }, [navigate, selectedSession]);

  const handleSwitchTab = useCallback((mode: ViewMode) => {
    navigate({ view: mode });
  }, [navigate]);

  // Cross-link: Live → Explore
  const handleExploreLink = useCallback((link: CrossLink) => {
    navigate({ view: "explore", sessionId: link.sessionId, ...(link.eventId ? { eventId: link.eventId } : {}) });
  }, [navigate]);

  return (
    <div className="h-screen flex flex-col bg-[#1a1b26] text-[#c0caf5]">
      {/* Header */}
      <header className="flex items-center justify-between px-4 py-2 bg-[#24283b] border-b border-[#2f3348]">
        <div className="flex items-center gap-4">
          <h1 className="text-lg font-semibold">Open Story</h1>
          <TabBar active={viewMode} onSwitch={handleSwitchTab} />
        </div>
        <div className="flex items-center gap-2 text-xs text-[#565f89]" data-testid="connection-status">
          <span className={`w-2 h-2 rounded-full ${color}`} />
          {label}
        </div>
      </header>

      {/* Live tab */}
      {viewMode === "live" && (
        <div className="flex flex-1 min-h-0">
          <Sidebar
            events={state.records}
            selectedSession={selectedSession}
            onSelectSession={handleSelectSession}
            focusAgentId={focusAgentId}
            onFocusAgent={setFocusAgentId}
            sessionLabels={state.sessionLabels}
            userFilter={userFilter}
            onUserFilterChange={handleUserFilterChange}
          />
          <div className="flex-1 min-w-0 flex flex-col">
            <SessionHeaderForLive
              sessionId={selectedSession}
              localUser={localInfo?.user ?? null}
            />
            <Timeline
              state$={state$}
              sessionFilter={selectedSession}
              agentFilter={focusAgentId}
              onExploreLink={handleExploreLink}
            />
          </div>
        </div>
      )}

      {/* Explore tab */}
      {viewMode === "explore" && (
        <ExploreView
          route={route}
          onNavigate={navigate}
        />
      )}

      {/* Story tab */}
      {viewMode === "story" && (
        <StoryView
          livePatterns={state.patterns}
          selectedSession={storySession}
          onSelectSession={(sid) => navigate({ view: "story", ...(sid ? { sessionId: sid } : {}) })}
        />
      )}

      {/* Users tab */}
      {viewMode === "users" && <UsersView onNavigate={navigate} />}
    </div>
  );
}

/**
 * Thin wrapper so the hook (which fetches `/api/sessions`) only fires
 * when the Live tab is mounted. Lifting `useSessionHeaderInfo` into
 * App's body would call it on every tab.
 */
function SessionHeaderForLive({
  sessionId,
  localUser,
}: {
  sessionId: string | null;
  localUser: string | null;
}) {
  const info = useSessionHeaderInfo(sessionId);
  return <SessionHeader session={info} localUser={localUser} />;
}
