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
  isReelPaused,
  type ReelPlayerEvent,
  type ReelPlayerState,
} from "@/lib/reel-player";
import { EventSpotlight } from "@/components/control/EventSpotlight";
import { TitleSpotlight } from "@/components/control/TitleSpotlight";
import { ReelBeatStage } from "@/components/reels/ReelBeatStage";
import { BeatInkLayer } from "@/components/reels/BeatInkLayer";
import { normalizeStopKind } from "@/lib/reel-visual";
import { normalizeReelToSlides, playerToSlideIndex, captionFor } from "@/lib/reel-slide";
import { absoluteTime, fullTimestamp } from "@/lib/time";
import { drawInteractive$, setDrawInteractive } from "@/streams/draw";
import { clearActiveBeatInk } from "@/streams/reel-annotate";

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
  const [annotating, setAnnotating] = useState(false);

  useEffect(() => {
    const sub = drawInteractive$().subscribe(setAnnotating);
    return () => sub.unsubscribe();
  }, []);

  // Leaving the player always ends annotate mode so the next surface is clickable.
  useEffect(() => {
    return () => setDrawInteractive(false);
  }, []);

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

  // Narration: speak (or caption-pace) only while *playing* (not paused).
  // Paused = freeze auto-advance so the human can click through / annotate.
  useEffect(() => {
    if ((state.phase !== "stop" && state.phase !== "opener") || !reel) return;
    if (state.paused) {
      window.speechSynthesis?.cancel();
      return;
    }
    const line =
      state.phase === "opener" ? reel.opener : reel.stops[state.index]?.line;
    if (!line) return;
    let fallback: number | undefined;
    let disposed = false;
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
      const ms = Math.max(3500, (line.split(/\s+/).length / 3) * 1000 + 2000);
      fallback = setTimeout(() => dispatch({ type: "ADVANCE" }), ms);
    }
    return () => {
      disposed = true;
      window.speechSynthesis?.cancel();
      if (fallback) clearTimeout(fallback);
    };
  }, [state, reel]);

  // Space = pause/resume when playing; ArrowRight/Left = click through.
  // When paused, Space also resumes (play). Esc exits.
  useEffect(() => {
    if (state.phase !== "stop" && state.phase !== "closer" && state.phase !== "opener") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.code === "Space" || e.key === " ") {
        e.preventDefault();
        dispatch({ type: "TOGGLE_PAUSE" });
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        dispatch({ type: "ADVANCE" });
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        dispatch({ type: "BACK" });
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

  // Unified slides: opener + body + closer → one list (standard format).
  const slideReel = normalizeReelToSlides(reel);
  const slideIndex =
    state.phase === "opener" || state.phase === "stop" || state.phase === "closer"
      ? playerToSlideIndex(
          slideReel.slides,
          state.phase,
          state.phase === "stop" ? state.index : 0,
        )
      : null;
  const slide = slideIndex != null ? slideReel.slides[slideIndex] : null;

  // Shared chrome for every slide kind (standard toolbar).
  const slideChrome =
    slideIndex != null && slide ? (
      <>
        <BeatInkLayer reelId={reel.id} beatIndex={slideIndex} />
        {!annotating && (
          <PlaybackClickSurface onAdvance={() => dispatch({ type: "ADVANCE" })} />
        )}
        <div
          className="fixed inset-x-0 bottom-0 z-[110] border-t border-[color:var(--divider)] bg-[color:var(--bg-surface)]/95 px-6 pb-4 pt-3 backdrop-blur-sm"
          data-testid="reels-caption-bar"
        >
          {captionFor(slide) && (
            <p
              className="mx-auto max-w-4xl cursor-pointer text-center text-lg leading-relaxed text-[color:var(--text)]"
              onClick={() => !annotating && dispatch({ type: "ADVANCE" })}
            >
              {captionFor(slide)}
            </p>
          )}
          <div className="mx-auto mt-3 flex max-w-4xl flex-wrap items-center justify-center gap-3">
            <button
              onClick={() => dispatch({ type: "BACK" })}
              className="rounded px-2 py-1 text-sm text-[color:var(--text-muted)] transition-colors hover:text-[color:var(--text)]"
              aria-label="Previous slide"
              data-testid="reels-back"
            >
              ‹ back
            </button>
            <button
              type="button"
              onClick={() => dispatch({ type: "TOGGLE_PAUSE" })}
              className={
                "rounded-full border px-3 py-1 text-xs font-medium transition-colors " +
                (isReelPaused(state)
                  ? "border-[color:var(--accent)] bg-[color:var(--accent)]/15 text-[color:var(--accent)]"
                  : "border-[color:var(--border)] text-[color:var(--text)] hover:border-[color:var(--accent)]")
              }
              aria-label={isReelPaused(state) ? "Play" : "Pause"}
              data-testid="reels-play-pause"
            >
              {isReelPaused(state) ? "▶ Play" : "⏸ Pause"}
            </button>
            <div className="flex items-center gap-1.5" data-testid="reels-progress">
              {slideReel.slides.map((s, i) => (
                <button
                  key={s.id}
                  onClick={() => {
                    // Map unified index → player JUMP (body only) or BACK/ADVANCE to ends.
                    if (s.role === "opener") {
                      dispatch({ type: "PLAY" });
                      return;
                    }
                    if (s.role === "closer") {
                      // jump to last body then advance — simpler: JUMP last stop
                      const bodyCount = reel.stops.length;
                      if (bodyCount > 0) {
                        dispatch({ type: "JUMP", index: bodyCount - 1 });
                        dispatch({ type: "ADVANCE" });
                      }
                      return;
                    }
                    const bodyIdx = slideReel.slides
                      .filter((x) => x.role === "body")
                      .findIndex((x) => x.id === s.id);
                    if (bodyIdx >= 0) dispatch({ type: "JUMP", index: bodyIdx });
                  }}
                  aria-label={`Go to slide ${i + 1}`}
                  data-testid={`reels-segment-${i}`}
                  className={
                    "h-1.5 w-8 rounded-full transition-colors " +
                    (i === slideIndex
                      ? "bg-[color:var(--accent)]"
                      : i < slideIndex
                        ? "bg-[color:var(--accent)]/40"
                        : "bg-[color:var(--divider)] hover:bg-[color:var(--text-muted)]")
                  }
                />
              ))}
            </div>
            <button
              onClick={() => dispatch({ type: "ADVANCE" })}
              className="rounded px-2 py-1 text-sm text-[color:var(--text-muted)] transition-colors hover:text-[color:var(--text)]"
              aria-label="Next slide"
              data-testid="reels-next"
            >
              next ›
            </button>
            <button
              type="button"
              onClick={() => setDrawInteractive(!annotating)}
              className={
                "rounded-full border px-3 py-1 text-xs font-medium transition-colors " +
                (annotating
                  ? "border-rose-500 bg-rose-500/20 text-rose-700 dark:text-rose-200"
                  : "border-[color:var(--accent)]/50 text-[color:var(--accent)] hover:bg-[color:var(--accent)]/10")
              }
              data-testid="reels-annotate"
              title="Ink is 1:1 with this slide"
            >
              {annotating ? "Done annotating" : "✎ Annotate slide"}
            </button>
            <button
              type="button"
              onClick={() => clearActiveBeatInk()}
              className="rounded px-2 py-1 text-[11px] text-[color:var(--text-muted)] hover:text-[color:var(--text)]"
              title="Clear ink on this slide only"
              data-testid="reels-clear-beat-ink"
            >
              Clear slide ink
            </button>
            <span className="text-[11px] tabular-nums text-[color:var(--text-muted)]">
              {slideIndex + 1} / {slideReel.slides.length}
              {slide.kind !== "title" ? ` · ${slide.kind}` : ""}
            </span>
          </div>
        </div>
      </>
    ) : null;

  if (state.phase === "opener" && reel.opener) {
    return (
      <>
        <TitleSpotlight message={reel.opener} onClose={exit} />
        {slideChrome}
      </>
    );
  }

  if (state.phase === "stop") {
    const stop = reel.stops[state.index];
    if (!stop) return null;
    const kind = normalizeStopKind(stop.kind);
    const stage =
      kind === "spotlight" && stop.sessionId && stop.eventId ? (
        <EventSpotlight
          sessionId={stop.sessionId}
          eventId={stop.eventId}
          clipAt={stop.clipAt}
          onClose={exit}
        />
      ) : (
        <ReelBeatStage stop={stop} onClose={exit} />
      );
    return (
      <>
        {stage}
        {slideChrome}
      </>
    );
  }

  if (state.phase === "closer" && reel.closer) {
    return (
      <>
        <TitleSpotlight message={reel.closer} onClose={exit} />
        {slideChrome}
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

