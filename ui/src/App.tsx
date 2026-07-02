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
import { OverviewView } from "@/components/overview/OverviewView";
import { SessionsCanvas } from "@/components/canvas/SessionsCanvas";
import { AskView } from "@/components/ask/AskView";
import { HeatmapView } from "@/components/heatmap/HeatmapView";
import { UsersView } from "@/components/users/UsersView";
import { AdminView } from "@/components/admin/AdminView";
import { SessionHeader, useSessionHeaderInfo } from "@/components/SessionHeader";
import { CommandPalette } from "@/components/command/CommandPalette";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { useRecents } from "@/hooks/use-recents";
import { useLocalInfo } from "@/hooks/use-local-info";
import { EMPTY_ENRICHED_STATE } from "@/streams/sessions";
import { interpretControl } from "@/lib/ui-control";
import { PresentBanner, type Presentation } from "@/components/control/PresentBanner";
import { AnnotationsOverlay } from "@/components/control/AnnotationsOverlay";
import { fetchAnnotations, mergeAnnotation, type Annotation } from "@/lib/annotations";
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
  const [drivenBy, setDrivenBy] = useState<string | null>(null);
  const [present, setPresent] = useState<Presentation | null>(null);
  const [annotations, setAnnotations] = useState<Annotation[]>([]);

  // Durable overlay annotations: load existing on mount, append live ones.
  useEffect(() => { fetchAnnotations().then(setAnnotations); }, []);
  useEffect(() => {
    const sub = wsMessages$().subscribe((msg) => {
      if (msg.kind === "annotation_added") setAnnotations((prev) => mergeAnnotation(prev, msg.annotation));
    });
    return () => sub.unsubscribe();
  }, []);

  // Agent-in-UI WRITE seam: react to `control` view-intents broadcast over the
  // WebSocket (an MCP/operator posts to /api/control). The UI is a sink — it
  // reacts; it never drives itself. Every drive is made visible ("driven by X"
  // + a dismissible present banner) so the mirror stays seizable, not a leash.
  useEffect(() => {
    const sub = wsMessages$().subscribe((msg) => {
      if (msg.kind !== "control") return;
      const issuer = typeof msg.issuer === "string" && msg.issuer ? msg.issuer : "an agent";
      const action = interpretControl(msg.action, msg.params);
      if (action?.type === "navigate") {
        navigate(action.route);
      } else if (action?.type === "present") {
        if (action.route) navigate(action.route);
        setPresent({ issuer, message: action.message, sessionIds: action.sessionIds, route: action.route });
      }
      setDrivenBy(issuer);
    });
    return () => sub.unsubscribe();
  }, [navigate]);
  useEffect(() => {
    if (!drivenBy) return;
    const t = setTimeout(() => setDrivenBy(null), 4000);
    return () => clearTimeout(t);
  }, [drivenBy]);
  const localInfo = useLocalInfo();
  const { sessions: allSessions } = useSessionsList();
  const { recentIds, record: recordRecent } = useRecents();

  // Record a session visit whenever it's opened in Explore (covers palette
  // jumps, cross-links, and "Open in Explore" from the Overview drill-in).
  useEffect(() => {
    if (route.view === "explore" && route.sessionId) recordRecent(route.sessionId);
  }, [route.view, route.sessionId, recordRecent]);

  // Derive view state from route
  const viewMode = route.view;
  const selectedSession = route.view === "live" ? (route.sessionId ?? null) : null;
  const storySession = route.view === "story" ? (route.sessionId ?? null) : null;
  // Live tab filters are owned by the URL — bookmarkable & shareable.
  const userFilter = route.view === "live" ? (route.userFilter ?? null) : null;
  const timeFilter =
    route.view === "live" ? (route.timeFilter ?? "all") : "all";

  const handleSelectSession = useCallback((sid: string | null) => {
    setFocusAgentId(null);
    // Preserve active filters when picking a session — clicking a
    // session inside a filtered view shouldn't clear the filters.
    navigate({
      view: "live",
      ...(sid ? { sessionId: sid } : {}),
      ...(userFilter ? { userFilter } : {}),
      ...(timeFilter !== "all" ? { timeFilter } : {}),
    });
  }, [navigate, userFilter, timeFilter]);

  const handleUserFilterChange = useCallback((user: string | null) => {
    navigate({
      view: "live",
      ...(selectedSession ? { sessionId: selectedSession } : {}),
      ...(user ? { userFilter: user } : {}),
      ...(timeFilter !== "all" ? { timeFilter } : {}),
    });
  }, [navigate, selectedSession, timeFilter]);

  const handleTimeFilterChange = useCallback((next: "1h" | "today" | "week" | "all") => {
    navigate({
      view: "live",
      ...(selectedSession ? { sessionId: selectedSession } : {}),
      ...(userFilter ? { userFilter } : {}),
      ...(next !== "all" ? { timeFilter: next } : {}),
    });
  }, [navigate, selectedSession, userFilter]);

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
        <div className="flex items-center gap-3">
          <button
            onClick={() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true }))}
            className="flex items-center gap-1.5 rounded border border-[#3b4261] px-2 py-1 text-[11px] text-[#565f89] hover:border-[#7aa2f7] hover:text-[#c0caf5] transition-colors"
            title="Command palette"
          >
            <span>Jump to…</span>
            <kbd className="rounded bg-[#1a1b26] px-1 text-[10px]">⌘K</kbd>
          </button>
          {drivenBy && (
            <div
              className="flex items-center gap-1.5 rounded border border-[#7aa2f7]/50 bg-[#7aa2f7]/10 px-2 py-1 text-[11px] text-[#7aa2f7] animate-pulse"
              data-testid="driven-by"
              title="An agent is driving this view. Click anywhere or navigate to take back the wheel."
            >
              <span>▸</span> driven by {drivenBy}
            </div>
          )}
          <div className="flex items-center gap-2 text-xs text-[#565f89]" data-testid="connection-status">
            <span className={`w-2 h-2 rounded-full ${color}`} />
            {label}
          </div>
        </div>
      </header>

      {/* Agent "present" banner — the write seam's message-to-you surface */}
      {present && (
        <PresentBanner present={present} onNavigate={navigate} onDismiss={() => setPresent(null)} />
      )}

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
            timeFilter={timeFilter}
            onTimeFilterChange={handleTimeFilterChange}
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

      {/* Overview tab */}
      {viewMode === "overview" && <OverviewView route={route} onNavigate={navigate} />}

      {/* Canvas tab */}
      {viewMode === "canvas" && <SessionsCanvas onNavigate={navigate} />}

      {/* Ask tab */}
      {viewMode === "ask" && <AskView onNavigate={navigate} />}

      {/* Heatmap tab */}
      {viewMode === "heatmap" && <HeatmapView onNavigate={navigate} />}

      {/* Users tab */}
      {viewMode === "users" && <UsersView onNavigate={navigate} />}

      {/* Admin tab */}
      {viewMode === "admin" && <AdminView />}

      {/* Durable overlay annotations (agent/person notes) */}
      <AnnotationsOverlay annotations={annotations} onNavigate={navigate} />

      {/* Global ⌘K command palette */}
      <CommandPalette sessions={allSessions} onNavigate={navigate} recentIds={recentIds} />
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
