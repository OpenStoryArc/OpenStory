/** ReelsView — top-level Reels tab.
 *
 * Two renders keyed off `route.reelId`:
 *   - no `reelId`: the reel list (title, stop count, created date, author),
 *     each row navigating into the player, plus a ▶ Play affordance that
 *     jumps straight into autoplay.
 *   - `reelId` set: the player. Fetches the reel, drives `reelPlayerReduce`
 *     (Task 4) through its stop/closer/done phases, rendering each stop with
 *     the existing `EventSpotlight` (Task-independent, control/ dir) plus a
 *     caption bar layered above it, and the closer with `TitleSpotlight`.
 *     Narration speaks each stop's line (Web Speech, caption-paced fallback
 *     when unavailable) and auto-advances when it ends.
 *
 * Read-only, mirror-not-a-leash: this view never mutates a reel, it only
 * plays one back.
 */

import { useCallback, useEffect, useReducer, useState } from "react";
import type { HashRoute } from "@/lib/hash-route";
import { fetchReel, fetchReels, type Reel, type ReelMeta } from "@/lib/reels-api";
import {
  reelPlayerReduce,
  initialReelPlayerState,
  type ReelPlayerEvent,
  type ReelPlayerState,
} from "@/lib/reel-player";
import { EventSpotlight } from "@/components/control/EventSpotlight";
import { TitleSpotlight } from "@/components/control/TitleSpotlight";
import { absoluteTime, fullTimestamp } from "@/lib/time";

interface ReelsViewProps {
  route: HashRoute;
  onNavigate: (route: HashRoute) => void;
}

export function ReelsView({ route, onNavigate }: ReelsViewProps) {
  if (route.reelId) {
    // Remount the player on reel change — a fresh reducer/effect lifecycle
    // per reel, rather than reconciling an ADVANCE-in-flight reel's stops.
    return <ReelPlayer key={route.reelId} route={route} onNavigate={onNavigate} />;
  }
  return <ReelsList onNavigate={onNavigate} />;
}

function ReelsList({ onNavigate }: { onNavigate: (route: HashRoute) => void }) {
  const [reels, setReels] = useState<ReelMeta[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchReels().then((r) => {
      if (!cancelled) setReels(r);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  if (reels === null) {
    return (
      <div
        className="flex-1 min-h-0 overflow-y-auto p-6 text-sm text-[color:var(--text-muted)]"
        data-testid="reels-loading"
      >
        Loading reels…
      </div>
    );
  }

  if (reels.length === 0) {
    return (
      <div
        className="flex-1 min-h-0 overflow-y-auto p-8 flex flex-col items-center justify-start"
        data-testid="reels-empty"
      >
        <div className="max-w-md text-center">
          <div
            className="w-16 h-16 mx-auto mb-4 rounded-full flex items-center justify-center text-2xl"
            style={{ backgroundColor: "#bb9af715", color: "#bb9af7" }}
            aria-hidden="true"
          >
            🎬
          </div>
          <h2 className="text-base font-semibold text-[color:var(--text)] mb-2">No reels yet</h2>
          <p className="text-sm text-[color:var(--text-muted)]">
            No reels yet. An agent can save one with the save_reel MCP tool.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 min-h-0 overflow-y-auto p-6" data-testid="reels-view">
      <div className="mb-4 text-xs text-[color:var(--text-muted)]">
        {reels.length} reel{reels.length === 1 ? "" : "s"}
      </div>
      <div className="flex flex-col gap-2">
        {reels.map((meta) => (
          <ReelRow key={meta.id} meta={meta} onNavigate={onNavigate} />
        ))}
      </div>
    </div>
  );
}

function ReelRow({ meta, onNavigate }: { meta: ReelMeta; onNavigate: (route: HashRoute) => void }) {
  return (
    <div
      className="flex items-center justify-between gap-3 rounded-xl border border-[color:var(--divider)] bg-[color:var(--bg-surface)] px-4 py-3 cursor-pointer transition-colors hover:border-[color:var(--accent)]/50"
      onClick={() => onNavigate({ view: "reels", reelId: meta.id })}
      data-testid={`reel-row-${meta.id}`}
    >
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold text-[color:var(--text)]">{meta.title}</div>
        <div className="mt-0.5 flex items-center gap-2 text-[11px] text-[color:var(--text-muted)]">
          <span>
            {meta.stopCount} stop{meta.stopCount === 1 ? "" : "s"}
          </span>
          <span>·</span>
          <span title={fullTimestamp(meta.created)}>{absoluteTime(meta.created)}</span>
          <span>·</span>
          <span>@{meta.author}</span>
        </div>
      </div>
      <button
        onClick={(e) => {
          e.stopPropagation();
          onNavigate({ view: "reels", reelId: meta.id, reelAutoplay: true });
        }}
        className="shrink-0 rounded-full border border-[color:var(--accent)]/40 px-3 py-1 text-xs font-medium text-[color:var(--accent)] transition-colors hover:bg-[color:var(--accent)]/10"
        data-testid={`reel-play-${meta.id}`}
      >
        ▶ Play
      </button>
    </div>
  );
}

function ReelPlayer({ route, onNavigate }: { route: HashRoute; onNavigate: (route: HashRoute) => void }) {
  const reelId = route.reelId as string;
  // undefined = loading, null = fetched but not found.
  const [reel, setReel] = useState<Reel | null | undefined>(undefined);

  const [state, dispatch] = useReducer(
    (s: ReelPlayerState, e: ReelPlayerEvent) =>
      reelPlayerReduce(s, e, {
        stopCount: reel?.stops.length ?? 0,
        hasCloser: !!reel?.closer,
        hasOpener: !!reel?.opener,
      }),
    initialReelPlayerState,
  );

  useEffect(() => {
    let cancelled = false;
    setReel(undefined);
    fetchReel(reelId).then((r) => {
      if (!cancelled) setReel(r);
    });
    return () => {
      cancelled = true;
    };
  }, [reelId]);

  const exit = useCallback(() => {
    dispatch({ type: "EXIT" });
    onNavigate({ view: "reels" });
  }, [onNavigate]);

  // Autoplay: once the reel loads, start it — deep-links from the ▶ Play
  // row (`reelAutoplay: true`) skip the idle landing screen.
  useEffect(() => {
    if (reel && route.reelAutoplay && state.phase === "idle") {
      dispatch({ type: "PLAY" });
    }
  }, [reel, route.reelAutoplay, state.phase]);

  // `done` is transient — return to the list route.
  useEffect(() => {
    if (state.phase === "done") onNavigate({ view: "reels" });
  }, [state.phase, onNavigate]);

  // Narration: speak (or caption-pace) the current stop's line, then
  // auto-advance. speechSynthesis is guarded for non-browser/test
  // environments; unmount (or stop change) cancels any in-flight speech.
  useEffect(() => {
    if ((state.phase !== "stop" && state.phase !== "opener") || !reel) return;
    // Opener narrates its own card text (the BLUF); stops narrate their line.
    const line =
      state.phase === "opener" ? reel.opener : reel.stops[state.index]?.line;
    if (!line) return;
    let fallback: number | undefined;
    // Some engines (observed in Chrome) fire the outgoing utterance's
    // `onend` on cancellation as a queued task — AFTER this effect's own
    // cleanup below has already run and returned (see the `cancel()` call
    // there). When that late `onend` fires, it belongs to a dead closure:
    // nothing will ever read or clear the `fallback` timer it schedules,
    // so a manual click/Space advance is silently followed ~2s later by a
    // phantom ADVANCE that skips a stop. `disposed` makes the late onend
    // a no-op instead of scheduling that orphaned timer.
    let disposed = false;
    // Note: `window.setTimeout`/`clearTimeout` inside the `"speechSynthesis"
    // in window` narrowing would type the `else` branch as `never` (Window's
    // DOM lib type declares speechSynthesis as always-present) — use the
    // ambient global timers instead so both branches type-check.
    if ("speechSynthesis" in window) {
      const u = new SpeechSynthesisUtterance(line);
      u.rate = 1.0;
      u.onend = () => {
        if (disposed) return;
        fallback = setTimeout(() => dispatch({ type: "ADVANCE" }), 2000);
      };
      window.speechSynthesis.cancel();
      window.speechSynthesis.speak(u);
    } else {
      // Caption-paced fallback: ~180 wpm reading time + 2s beat.
      const ms = Math.max(3500, (line.split(/\s+/).length / 3) * 1000 + 2000);
      fallback = setTimeout(() => dispatch({ type: "ADVANCE" }), ms);
    }
    return () => {
      disposed = true;
      window.speechSynthesis?.cancel();
      if (fallback) clearTimeout(fallback);
    };
  }, [state, reel]);

  // Space advances; Esc exits. Click semantics need a deliberate seam:
  // EventSpotlight/TitleSpotlight wire their OWN backdrop click to
  // `onClose`, so passing `onClose={exit}` (needed so their internal Esc
  // listener exits, not advances — see the click-surface comment below)
  // means a plain click on the spotlight body would incorrectly exit the
  // reel instead of advancing it. Space is unaffected by that trap — it's
  // handled entirely here, never routed through the child's `onClose`.
  useEffect(() => {
    if (state.phase !== "stop" && state.phase !== "closer" && state.phase !== "opener") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.code === "Space" || e.key === " ") {
        e.preventDefault();
        dispatch({ type: "ADVANCE" });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.phase]);

  if (reel === undefined) {
    return (
      <div
        className="flex-1 min-h-0 overflow-y-auto p-6 text-sm text-[color:var(--text-muted)]"
        data-testid="reels-player-loading"
      >
        Loading reel…
      </div>
    );
  }

  if (reel === null) {
    return (
      <div className="flex-1 min-h-0 overflow-y-auto p-8 text-center" data-testid="reels-player-missing">
        <p className="text-sm text-[color:var(--text-muted)]">Reel not found.</p>
        <button
          onClick={() => onNavigate({ view: "reels" })}
          className="mt-3 text-sm text-[color:var(--accent)] hover:underline"
        >
          ← Back to reels
        </button>
      </div>
    );
  }

  if (state.phase === "idle") {
    return (
      <div
        className="flex flex-1 min-h-0 flex-col items-center justify-center overflow-y-auto p-8 text-center"
        data-testid="reels-player-idle"
      >
        <div className="max-w-md">
          <h2 className="mb-1 text-lg font-semibold text-[color:var(--text)]">{reel.title}</h2>
          <p className="mb-4 text-xs text-[color:var(--text-muted)]">
            {reel.stops.length} stop{reel.stops.length === 1 ? "" : "s"} · @{reel.author}
          </p>
          <button
            onClick={() => dispatch({ type: "PLAY" })}
            className="rounded-full bg-[color:var(--accent)] px-4 py-2 text-sm font-medium text-[color:var(--bg)] transition-opacity hover:opacity-90"
            data-testid="reels-player-play"
          >
            ▶ Play
          </button>
          <div className="mt-3">
            <button
              onClick={() => onNavigate({ view: "reels" })}
              className="text-xs text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
            >
              ← Back to reels
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (state.phase === "opener" && reel.opener) {
    return (
      <>
        <TitleSpotlight message={reel.opener} onClose={exit} />
        <PlaybackClickSurface onAdvance={() => dispatch({ type: "ADVANCE" })} />
      </>
    );
  }

  if (state.phase === "stop") {
    const stop = reel.stops[state.index];
    if (!stop) return null; // defensive — reducer keeps index in [0, stopCount)
    return (
      <>
        <EventSpotlight sessionId={stop.sessionId} eventId={stop.eventId} clipAt={stop.clipAt} onClose={exit} />
        <PlaybackClickSurface onAdvance={() => dispatch({ type: "ADVANCE" })} />
        {/* Caption bar rides above the click surface (z-[60] > z-[55]) so it
         *  stays readable and clickable while the spotlight is open — its own
         *  onClick also advances, which is consistent (never double-fires:
         *  a click only ever hits ONE topmost element per the browser's hit
         *  test, so a caption-bar click never reaches the surface beneath). */}
        <div
          className="fixed inset-x-0 bottom-0 z-[60] flex cursor-pointer items-center justify-center gap-3 border-t border-[color:var(--divider)] bg-[color:var(--bg-surface)]/95 px-6 py-3 backdrop-blur-sm"
          onClick={() => dispatch({ type: "ADVANCE" })}
          data-testid="reels-caption-bar"
        >
          <span className="max-w-3xl text-center text-sm text-[color:var(--text)]">{stop.line}</span>
          <span className="shrink-0 text-[11px] tabular-nums text-[color:var(--text-muted)]">
            {state.index + 1} / {reel.stops.length}
          </span>
        </div>
      </>
    );
  }

  if (state.phase === "closer" && reel.closer) {
    return (
      <>
        <TitleSpotlight message={reel.closer} onClose={exit} />
        <PlaybackClickSurface onAdvance={() => dispatch({ type: "ADVANCE" })} />
      </>
    );
  }

  // phase === "done" (or a closer-less closer edge case) — the effect above
  // navigates back to the list; render nothing for this frame.
  return null;
}

/** Click-to-advance contract for reel playback: click anywhere on the
 *  playback surface = ADVANCE, Esc = EXIT.
 *
 *  EventSpotlight and TitleSpotlight both wire their OWN full-screen
 *  backdrop to `onClick={onClose}` — correct for their normal, non-reel use
 *  (a click dismisses the spotlight), but wrong here: during reel playback
 *  a click should move to the next stop, not leave the reel. We can't just
 *  pass `onClose={advance}` instead of `onClose={exit}` — both components
 *  also route their internal Esc-keydown listener through that same
 *  `onClose` prop, so swapping it would make Esc advance instead of exit
 *  (the interaction trap this component exists to avoid). And we can't
 *  safely edit EventSpotlight/TitleSpotlight's click behavior in place —
 *  they're shared, used elsewhere (agent-driven spotlight/present control
 *  actions) with the click-to-dismiss semantics intact.
 *
 *  So instead: `onClose={exit}` stays wired for Esc, and this component lays
 *  a transparent, fixed, full-viewport hit-target on top of the spotlight's
 *  backdrop (z-[55], above its z-50 backdrop) that intercepts the click
 *  before it ever reaches the spotlight's own `onClick={onClose}` handler —
 *  the browser only dispatches a click to the topmost element under the
 *  pointer, so the spotlight's backdrop click handler never fires while
 *  this surface is stacked over it. Esc is unaffected: keydown listeners
 *  aren't scoped by stacking context, so the spotlight's own Esc listener
 *  still reaches `onClose={exit}` directly. */
function PlaybackClickSurface({ onAdvance }: { onAdvance: () => void }) {
  return (
    <div
      className="fixed inset-0 z-[55] cursor-pointer"
      onClick={onAdvance}
      aria-hidden="true"
      data-testid="reels-playback-surface"
    />
  );
}
