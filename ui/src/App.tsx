import { useEffect, useMemo, useState, useCallback, useRef } from "react";
import { connect, wsMessages$ } from "@/streams/connection";
import { buildSessionState$ } from "@/streams/sessions";
import { useConnectionStatus } from "@/hooks/use-connection-status";
import { useObservable } from "@/hooks/use-observable";
import { useHashRoute } from "@/hooks/use-hash-route";
import { Timeline } from "@/components/Timeline";
import { Sidebar } from "@/components/Sidebar";
import { TabBar } from "@/components/layout/TabBar";
import { TextSizeControl } from "@/components/layout/TextSizeControl";
import { ThemeToggle } from "@/components/layout/ThemeToggle";
import { ExploreView } from "@/components/explore/ExploreView";
import { StoryView } from "@/components/story/StoryView";
import { SessionsCanvas } from "@/components/canvas/SessionsCanvas";
import { ReelsView } from "@/components/reels/ReelsView";
import { AskView } from "@/components/ask/AskView";
import { UsersView } from "@/components/users/UsersView";
import { AdminView } from "@/components/admin/AdminView";
import { SessionHeader, useSessionHeaderInfo } from "@/components/SessionHeader";
import { CommandPalette } from "@/components/command/CommandPalette";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { useRecents } from "@/hooks/use-recents";
import { useLocalInfo } from "@/hooks/use-local-info";
import { usePersistedFlag } from "@/hooks/use-persisted-flag";
import { EMPTY_ENRICHED_STATE } from "@/streams/sessions";
import { interpretControl, resolveDrawScope, type UIControlAction } from "@/lib/ui-control";
import { injectControl } from "@/streams/control";
import {
  attentionFromRoute,
  materializeAttention,
  realizeIntent,
  type AttentionPorts,
} from "@/lib/attention";
import { commitAttention, syncAttentionFromRoute } from "@/streams/attention";
import {
  normalizeApplyOpen,
  normalizeAgentOpen,
  type ControlStep,
  type NavigateToParams,
} from "@/lib/nav-path";
import { PresentBanner, type Presentation } from "@/components/control/PresentBanner";
import { EventSpotlight } from "@/components/control/EventSpotlight";
import { TitleSpotlight } from "@/components/control/TitleSpotlight";
import { AnnotationsOverlay } from "@/components/control/AnnotationsOverlay";
import { DrawOverlay } from "@/components/draw/DrawOverlay";
import { DrawInkChip } from "@/components/draw/DrawInkChip";
import { DrawView } from "@/components/draw/DrawView";
import { routeGlassKey } from "@/lib/glass-ink";
import { commitGlassInkIntent } from "@/streams/glass-ink";
import {
  commitDraw,
  drawInteractive$,
  getDrawInteractive,
  getDrawScene,
  setPenSceneReporter,
} from "@/streams/draw";
import { fetchAnnotations, mergeAnnotation, removeAnnotation, deleteAnnotation, type Annotation } from "@/lib/annotations";
import {
  glassInkInteraction,
  interactionFromRoute,
  postInteraction,
  postInteractionWithLayoutEyes,
} from "@/lib/interaction";
import { penSceneToWire, type PenSceneWire } from "@/lib/pen-eyes";
import type { InteractionLayout } from "@/lib/interaction";
import type { ViewMode, CrossLink } from "@/lib/navigation";
import { switchTabRoute } from "@/lib/navigation";

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
  // Always-current route for callbacks that must read where the human IS now
  // (control appliers, pen eyes) rather than where they were when memoized.
  const routeRef = useRef(route);
  routeRef.current = route;
  const [focusAgentId, setFocusAgentId] = useState<string | null>(null);
  const [drivenBy, setDrivenBy] = useState<string | null>(null);
  const [present, setPresent] = useState<Presentation | null>(null);
  // Event Spotlight (presentation mode): one event full-screen, the rest dimmed.
  const [spotlight, setSpotlight] = useState<{ sessionId: string; eventId: string; clipAt?: string } | null>(null);
  // Title card: the message itself fills the screen (demo openers/closers).
  const [titleCard, setTitleCard] = useState<string | null>(null);
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  // Live tab: the sessions sidebar folds away — a clear labeled control, persisted.
  const [liveSidebar, setLiveSidebar] = usePersistedFlag("os.live.sidebar", true);

  // Keep reactive Attention spine in sync with the bookmarkable hash.
  useEffect(() => {
    syncAttentionFromRoute(route);
  }, [route]);

  const attentionPorts: AttentionPorts = useMemo(
    () => ({
      navigate,
      setSpotlight,
      setTitleCard,
      injectControl,
    }),
    [navigate],
  );

  // Durable overlay annotations: load existing on mount, append live ones.
  useEffect(() => { fetchAnnotations().then(setAnnotations); }, []);
  useEffect(() => {
    const sub = wsMessages$().subscribe((msg) => {
      if (msg.kind === "annotation_added") setAnnotations((prev) => mergeAnnotation(prev, msg.annotation));
      else if (msg.kind === "annotation_removed") setAnnotations((prev) => removeAnnotation(prev, msg.id));
    });
    return () => sub.unsubscribe();
  }, []);

  // Apply one interpreted control action (navigate / present / set / …).
  // Shared by single-intent drives and navigate_to sequences.
  const applyControlAction = useCallback(
    (action: UIControlAction, issuer: string) => {
      if (action.type === "navigate") {
        navigate(action.route);
        setSpotlight(null);
        setTitleCard(null);
      } else if (action.type === "present") {
        if (action.route) {
          navigate(action.route);
          setSpotlight(null);
          setTitleCard(null);
        }
        setPresent({
          issuer,
          message: action.message,
          sessionIds: action.sessionIds,
          route: action.route,
        });
        if (action.message.trim()) {
          postInteraction({
            kind: "navigate",
            view: action.route?.view ?? "live",
            ...(action.route?.sessionId ? { session_id: action.route.sessionId } : {}),
            present_message: action.message,
          });
        }
      } else if (action.type === "spotlight") {
        setSpotlight({
          sessionId: action.sessionId,
          eventId: action.eventId,
          clipAt: action.clipAt,
        });
        setTitleCard(null);
        postInteraction({
          kind: "navigate",
          view: "spotlight",
          session_id: action.sessionId,
          eventId: action.eventId,
          spotlight: true,
        });
      } else if (action.type === "title") {
        setTitleCard(action.message);
        setSpotlight(null);
        postInteraction({
          kind: "navigate",
          view: "title",
          present_message: action.message,
          spotlight: true,
        });
      } else if (action.type === "toggle" && action.target === "spotlight") {
        if (action.value === "off") {
          setSpotlight(null);
          setTitleCard(null);
        }
      } else if (action.type === "draw") {
        // Agent pen — ui.* ink only (never mutates observed history).
        // Where the ink lands (resolveDrawScope): explicit reelId+beatIndex →
        // beat ink; scope:"board" → the Draw tab's paper; default "here" → the
        // human's current glass context (the board when the Draw tab is open).
        const scope = resolveDrawScope(
          {
            ...(action.scope ? { scope: action.scope } : {}),
            ...(action.reelId != null ? { reelId: action.reelId } : {}),
            ...(action.beatIndex != null ? { beatIndex: action.beatIndex } : {}),
          },
          routeGlassKey(routeRef.current),
        );
        if (scope.target === "beat") {
          void import("@/streams/reel-annotate").then(
            ({ commitBeatInkIntent }) => {
              const ink = commitBeatInkIntent({
                reelId: scope.reelId,
                beatIndex: scope.beatIndex,
                clear: action.clear,
                strokes: action.strokes,
                mode: action.mode,
              });
              void import("@/lib/reel-annotate").then(({ beatInkToWire }) => {
                postInteraction({
                  kind: "navigate",
                  view: "reels",
                  reelId: scope.reelId,
                  beatIndex: scope.beatIndex,
                  annotate: false,
                  beatInk: beatInkToWire(ink, { interactive: false }),
                });
              });
            },
          );
          return;
        }
        // Recipes resolve async (edge-trace / geometric) so the agent can "really draw".
        const label = action.label ?? issuer;
        /** Land resolved strokes on the surface the scope named. */
        const landInk = (
          strokes: readonly unknown[],
          opts: { clear?: boolean; mode?: "append" | "replace" },
        ) => {
          if (scope.target === "board") {
            commitDraw({
              clear: opts.clear,
              strokes,
              visible: action.visible,
              label,
              mode: opts.mode,
            });
          } else {
            const ink = commitGlassInkIntent({
              key: scope.key,
              clear: opts.clear,
              strokes,
              mode: opts.mode,
            });
            // Agent eyes: board ink reports via the pen-scene reporter and beat
            // ink via beatInk — glass ink needs its own report or an agent that
            // just drew here cannot see its own stroke land.
            postInteraction(
              glassInkInteraction(routeRef.current, scope.key, ink.strokes.length),
            );
          }
        };
        if (action.recipe) {
          void (async () => {
            try {
              const { flowerStrokes, smileyStrokes } = await import("@/lib/draw");
              const { geometricMaxStrokes, portraitInkStrokes } = await import(
                "@/lib/draw-portrait"
              );
              let strokes = action.strokes as import("@/lib/draw").DrawStroke[];
              if (action.recipe === "smiley") {
                strokes = smileyStrokes();
              } else if (action.recipe === "flower") {
                strokes = flowerStrokes();
              } else if (action.recipe === "geometric-max") {
                strokes = geometricMaxStrokes();
              } else if (action.recipe === "layout-ring") {
                // Layout eyes → ring the focused glass target (event/session/spotlight).
                const { collectLayoutEyes, ringStrokesForRect } = await import(
                  "@/lib/layout-eyes"
                );
                const preferId = action.preferId;
                const eyes = collectLayoutEyes({ preferId });
                if (eyes.focus) {
                  strokes = ringStrokesForRect(eyes.focus.rect, {
                    label: `${eyes.focus.kind} ${eyes.focus.id.slice(0, 12)}`,
                    stroke: "#e11d48",
                  });
                } else {
                  strokes = [
                    {
                      type: "text",
                      x: 0.5,
                      y: 0.5,
                      text: "layout eyes: no target on glass",
                      fill: "#e11d48",
                      fontSize: 16,
                    },
                  ];
                }
              } else if (action.recipe === "edge-portrait") {
                const href =
                  action.href ??
                  "https://github.com/maxglassie.png";
                try {
                  strokes = await portraitInkStrokes(href, {
                    caption: action.label ?? "Max Glassie",
                  });
                } catch {
                  // CORS / load failure → pure geometric ink (still a drawing)
                  strokes = geometricMaxStrokes();
                }
              }
              landInk(strokes, { clear: true, mode: "replace" });
            } catch {
              landInk(action.strokes, { clear: action.clear, mode: action.mode });
            }
          })();
        } else {
          landInk(action.strokes, { clear: action.clear, mode: action.mode });
        }
      } else if (action.type === "set" && action.target === "story.details") {
        const p = action.params;
        const sessionId =
          typeof p.sessionId === "string" && p.sessionId.trim()
            ? p.sessionId.trim()
            : route.sessionId;
        const eventId =
          typeof p.eventId === "string" && p.eventId.trim()
            ? p.eventId.trim()
            : route.eventId;
        const open =
          p.open === true ||
          p.open === "true" ||
          p.open === 1 ||
          p.value === "open" ||
          p.value === "on" ||
          p.value === true;
        if (sessionId && eventId && open) {
          const applyOpen = normalizeApplyOpen(
            p.applyOpen as NavigateToParams["applyOpen"],
          );
          const agentOpen = normalizeAgentOpen(
            Array.isArray(p.agentOpen)
              ? (p.agentOpen as string[])
              : typeof p.agentOpen === "string"
                ? (p.agentOpen as string).split(",")
                : undefined,
          );
          const evalOpen =
            p.evalOpen === true ||
            p.evalOpen === "true" ||
            applyOpen !== undefined ||
            agentOpen !== undefined;
          navigate({
            view: "story",
            sessionId,
            eventId,
            storyDetails: true,
            storyEvalOpen: evalOpen,
            storyEventsOpen: p.eventsOpen === true || p.eventsOpen === "true",
            ...(applyOpen !== undefined ? { storyApplyOpen: applyOpen } : {}),
            ...(agentOpen !== undefined ? { storyAgentOpen: agentOpen } : {}),
          });
          setSpotlight(null);
          setTitleCard(null);
        } else if (sessionId && eventId && !open) {
          navigate({
            view: "story",
            sessionId,
            eventId,
            storyDetails: false,
            storyEvalOpen: false,
            storyEventsOpen: false,
            storyApplyOpen: undefined,
            storyAgentOpen: undefined,
          });
        }
      } else if (
        action.type === "toggle" &&
        action.target === "story.details" &&
        (action.value === "open" || action.value === "on")
      ) {
        if (route.view === "story" && route.sessionId && route.eventId) {
          navigate({
            view: "story",
            sessionId: route.sessionId,
            eventId: route.eventId,
            storyDetails: true,
          });
        }
      }
      // toggle/set for canvas etc. flow via controlActions$ → view sinks
    },
    [navigate, route.sessionId, route.eventId, route.view],
  );

  /** Run a planned multi-step drive (navigate_to) with a short gap so sinks land.
   *  Toggle/set for canvas etc. are injectControl'd so controlActions$ sinks fire. */
  const runControlSequence = useCallback(
    async (steps: readonly ControlStep[], issuer: string) => {
      for (const step of steps) {
        const action = interpretControl(step.action, step.params);
        if (!action || action.type === "navigate_sequence") continue;
        // Route-level actions stay in App; component knobs go through control$.
        if (action.type === "toggle" || action.type === "set") {
          injectControl(step.action, step.params, issuer);
          // story.details is also handled in App (hash)
          if (action.type === "set" && action.target === "story.details") {
            applyControlAction(action, issuer);
          }
        } else {
          applyControlAction(action, issuer);
        }
        await new Promise((r) => setTimeout(r, 150));
      }
    },
    [applyControlAction],
  );

  // Agent-in-UI WRITE seam: control intents → pure Attention fold → materialize.
  // Prefer realizeIntent (denotational) for navigate_to; fall back to step sequence.
  useEffect(() => {
    const sub = wsMessages$().subscribe((msg) => {
      if (msg.kind !== "control") return;
      const issuer = typeof msg.issuer === "string" && msg.issuer ? msg.issuer : "an agent";

      // High-level hand: Intent → Attention → pixels (artful expression of data)
      if (msg.action === "navigate_to") {
        const intent = (msg.params ?? {}) as NavigateToParams;
        const base = attentionFromRoute(route);
        const next = realizeIntent(base, intent, interpretControl);
        if (next) {
          commitAttention(next);
          materializeAttention(next, attentionPorts, issuer);
          setDrivenBy(issuer);
          return;
        }
      }

      const action = interpretControl(msg.action, msg.params);
      if (!action) return;
      if (action.type === "navigate_sequence") {
        // Fallback multi-step path (canvas sinks + injectControl)
        void runControlSequence(action.steps, issuer);
      } else {
        applyControlAction(action, issuer);
      }
      setDrivenBy(issuer);
    });
    return () => sub.unsubscribe();
  }, [applyControlAction, runControlSequence, route, attentionPorts]);
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

  // Pen + layout snapshots merge so the latest ui_state keeps both eyes.
  const lastPenRef = useRef<PenSceneWire | null>(null);
  const lastLayoutRef = useRef<InteractionLayout | null>(null);

  // Read half of the agent-in-UI seam: report where the human goes. Each route
  // change becomes a first-class interaction event in OpenStory (the viewing
  // session) — projectable to ui_state, replayable. After paint, layout eyes
  // measure glass targets (event/session rects) so the pen can aim.
  // Re-attach last pen scene so navigating does not blank pen eyes.
  useEffect(() => {
    const base = interactionFromRoute(route);
    const withPen = lastPenRef.current ? { ...base, pen: lastPenRef.current } : base;
    return postInteractionWithLayoutEyes(withPen, {
      onLayout: (layout) => {
        lastLayoutRef.current = layout;
      },
    });
  }, [route]);

  // Pen eyes: when ink changes, publish a bounded scene snapshot on ui-state
  // (debounced). Agents read it via where_is_user / GET /api/ui-state → pen.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const report = (scene: import("@/lib/draw").DrawScene) => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        const interactive = getDrawInteractive();
        const pen = penSceneToWire(scene, { interactive });
        lastPenRef.current = pen.empty && !interactive ? null : pen;
        postInteraction({
          ...interactionFromRoute(routeRef.current),
          ...(lastLayoutRef.current ? { layout: lastLayoutRef.current } : {}),
          pen,
          annotate: interactive,
        });
      }, 280);
    };
    setPenSceneReporter(report);
    const current = getDrawScene();
    if (current.strokes.length > 0 || getDrawInteractive()) report(current);
    return () => {
      if (timer) clearTimeout(timer);
      setPenSceneReporter(null);
    };
  }, []);

  // Annotate mode on/off is a first-class ui journey event (pen.interactive + annotate).
  useEffect(() => {
    const sub = drawInteractive$().subscribe((on) => {
      const scene = getDrawScene();
      const pen = penSceneToWire(scene, { interactive: on });
      lastPenRef.current = pen.empty && !on ? null : pen;
      postInteraction({
        ...interactionFromRoute(routeRef.current),
        ...(lastLayoutRef.current ? { layout: lastLayoutRef.current } : {}),
        pen,
        annotate: on,
      });
    });
    return () => sub.unsubscribe();
  }, []);

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
    // Carry the selected session across tabs (Live→Story keeps the session).
    navigate(switchTabRoute(route, mode));
  }, [navigate, route]);

  // Cross-link: Live → Explore
  const handleExploreLink = useCallback((link: CrossLink) => {
    navigate({ view: "explore", sessionId: link.sessionId, ...(link.eventId ? { eventId: link.eventId } : {}) });
  }, [navigate]);

  return (
    <div className="h-screen flex flex-col bg-[color:var(--bg)] text-[color:var(--text)]">
      {/* Header */}
      <header className="flex items-center justify-between px-4 py-2 bg-[color:var(--bg-surface)]">
        <div className="flex min-w-0 flex-1 items-center gap-4">
          <h1 className="shrink-0 text-lg font-semibold">Open Story</h1>
          <TabBar active={viewMode} onSwitch={handleSwitchTab} />
        </div>
        <div className="flex shrink-0 items-center gap-3">
          <ThemeToggle />
          <TextSizeControl />
          <button
            onClick={() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true }))}
            className="flex items-center gap-1.5 rounded border border-[color:var(--border)] px-2 py-1 text-[11px] text-[color:var(--text-muted)] hover:border-[color:var(--accent)] hover:text-[color:var(--text)] transition-colors"
            title="Command palette"
          >
            <span>Jump to…</span>
            <kbd className="rounded bg-[color:var(--bg)] px-1 text-[10px]">⌘K</kbd>
          </button>
          {drivenBy && (
            <div
              className="flex items-center gap-1.5 rounded border border-[color:var(--accent)]/50 bg-[color:var(--accent)]/10 px-2 py-1 text-[11px] text-[color:var(--accent)] animate-pulse"
              data-testid="driven-by"
              title="An agent is driving this view. Click anywhere or navigate to take back the wheel."
            >
              <span>▸</span> driven by {drivenBy}
            </div>
          )}
          <div className="flex items-center gap-2 text-xs text-[color:var(--text-muted)]" data-testid="connection-status">
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
          {liveSidebar ? (
            <div className="relative flex min-h-0 shrink-0">
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
              {/* Clear, well-sized collapse control — pinned bottom of the rail. */}
              <button
                onClick={() => setLiveSidebar(false)}
                className="absolute bottom-3 left-3 z-10 rounded-lg border border-[color:var(--border)] bg-[color:var(--bg-surface)] px-3 py-1.5 text-[length:var(--fs-body)] font-medium text-[color:var(--text-muted)] shadow-card transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
                title="Collapse the sessions sidebar"
              >
                ◂ hide sessions
              </button>
            </div>
          ) : (
            <div className="flex min-h-0 shrink-0 flex-col border-r border-[color:var(--divider)] bg-[color:var(--bg-surface)] px-1.5 pt-3">
              <button
                onClick={() => setLiveSidebar(true)}
                className="rounded-lg border border-[color:var(--border)] bg-[color:var(--bg-surface)] px-2 py-2 text-[length:var(--fs-body)] font-medium text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)] [writing-mode:vertical-rl]"
                title="Show the sessions sidebar"
              >
                ▸ sessions
              </button>
            </div>
          )}
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
          eventId={route.eventId}
          storyDetails={route.storyDetails}
          storyEvalOpen={route.storyEvalOpen}
          storyEventsOpen={route.storyEventsOpen}
          storyApplyOpen={route.storyApplyOpen}
          storyAgentOpen={route.storyAgentOpen}
          onOpenEvent={(sid, eid) => navigate({ view: "explore", sessionId: sid, eventId: eid })}
        />
      )}

      {/* Canvas tab */}
      {viewMode === "canvas" && <SessionsCanvas onNavigate={navigate} />}

      {/* Reels tab */}
      {viewMode === "reels" && <ReelsView route={route} onNavigate={navigate} />}

      {/* Draw tab — free canvas + agent pen demos */}
      {viewMode === "draw" && <DrawView />}

      {/* Ask tab */}
      {viewMode === "ask" && <AskView onNavigate={navigate} />}

      {/* Users tab */}
      {viewMode === "users" && <UsersView onNavigate={navigate} />}

      {/* Admin tab */}
      {viewMode === "admin" && <AdminView />}

      {/* Event Spotlight — presentation mode over everything (Esc / click closes) */}
      {spotlight && (
        <EventSpotlight
          sessionId={spotlight.sessionId}
          eventId={spotlight.eventId}
          clipAt={spotlight.clipAt}
          onClose={() => setSpotlight(null)}
        />
      )}

      {/* Title card — the message itself as the full-screen shot */}
      {titleCard && <TitleSpotlight message={titleCard} onClose={() => setTitleCard(null)} />}

      {/* Durable overlay annotations (agent/person notes) */}
      <AnnotationsOverlay
        annotations={annotations}
        onNavigate={navigate}
        onRemove={(id) => { setAnnotations((prev) => removeAnnotation(prev, id)); void deleteAnnotation(id); }}
      />

      {/* Attention canvas — this context's glass ink (ui.* only; pointer-events none) */}
      <DrawOverlay route={route} suppress={viewMode === "draw"} />
      {/* Chip on history tabs: annotate here / open the Draw paper / clear */}
      {viewMode !== "draw" && (
        <DrawInkChip
          route={route}
          onOpenDraw={() => navigate({ view: "draw" })}
        />
      )}

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
