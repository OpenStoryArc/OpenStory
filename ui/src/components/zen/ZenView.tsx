/** ZenView — the calm room.
 *
 *  The story, one sentence at a time: as turns complete, their narrative
 *  sentences (verb + object, with an optional "because") breathe in at the
 *  bottom of a blank canvas. Markdown renders inline; long absolute paths are
 *  shortened to their last two segments (a full /Users/... path is not zen).
 *  Watches ONE person at a time. Falls back to one-line event bullets for
 *  sessions that have no sentences yet.
 *
 *  Zen Replay: a quiet "▶ replay" control auto-plays a past session's
 *  sentences from the beginning — one at a time, same breathe-in animation,
 *  synced smooth scroll, a hairline progress bar, and a speed control. See
 *  the "Replay" section below.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type { WireRecord, PatternView } from "@/types/wire-record";
import { toTimelineRows, type TimelineCategory } from "@/lib/timeline";
import { categorizeTurn, type StoryCategory } from "@/lib/story";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { fetchSessionSentences } from "@/lib/story-api";
import { compactTime, fullTimestamp, relativeTime } from "@/lib/time";
import { EmptyState } from "@/components/ui/EmptyState";
import { Markdown } from "@/components/ui/Markdown";
import { cn } from "@/lib/cn";
import { controlActions$ } from "@/streams/control";

const KIND_COLOR: Record<TimelineCategory, string> = {
  prompt: "var(--accent)",
  response: "var(--purple)",
  thinking: "var(--green)",
  tool: "var(--cyan)",
  result: "var(--cyan)",
  system: "var(--text-muted)",
  error: "var(--red)",
  turn: "var(--border)",
};

const STORY_COLOR: Record<StoryCategory, string> = {
  pure_text: "var(--green)",
  tool_use: "var(--orange)",
  thinking: "var(--purple)",
  delegation: "var(--cyan)",
  error: "var(--red)",
};

/** How much history the room holds — enough to scroll, never a wall. */
const ZEN_ROWS = 400;

/** Compose a FLOWING sentence from a turn pattern — fuller than the Story
 *  tab's clipped headline: verb + object, with the "because" clause woven
 *  inline (up to ~220 chars) so the sentence structure actually reads. */
function zenSentence(p: PatternView): { text: string; because: string | null } {
  const m = p.metadata ?? {};
  const verb = typeof m.verb === "string" ? m.verb.trim() : "";
  const object = typeof m.object === "string" ? m.object.trim() : "";
  const text = [verb, object].filter(Boolean).join(" ").trim() || p.label || "…";
  let because: string | null = null;
  if (typeof m.adverbial === "string" && m.adverbial.trim()) {
    const raw = m.adverbial.trim().replace(/^"|"$/g, "").replace(/\s+/g, " ");
    because = raw.slice(0, 220).trim() || null;
    if (because && raw.length > 220) because += "…";
  }
  return { text, because };
}

/** `/Users/max/projects/OpenStory/rs/src/watcher.rs` → `src/watcher.rs`.
 *  Absolute paths read as noise in a calm room; keep the last two segments. */
export function shortenPaths(text: string): string {
  return text.replace(
    /(?:\/[\w.@~+-]+){3,}/g,
    (path) => {
      const segs = path.split("/").filter(Boolean);
      return segs.slice(-2).join("/");
    },
  );
}

interface SessionLike {
  session_id: string;
  user?: string | null;
  last_event?: string | null;
  label?: string | null;
}

// ═══════════════════════════════════════════════════════════════════
// Shared sentence row — used by BOTH the live stream and Zen Replay, so
// composing, path-shortening, category-dot color, markdown, the
// "because" clause, and click-through to Explore never drift apart.
// ═══════════════════════════════════════════════════════════════════

function ZenSentenceRow({
  id,
  text,
  because,
  color,
  time,
  onClick,
  focused,
}: {
  id: string;
  text: string;
  because: string | null;
  color: string;
  /** Replay-only: a compactTime or "n / total" label. Live rows never pass
   *  this — omitting it keeps the live row's DOM byte-for-byte unchanged. */
  time?: string | null;
  onClick?: () => void;
  focused?: boolean;
}) {
  return (
    <div
      id={id}
      onClick={onClick}
      title={onClick ? "Open this moment in Explore" : undefined}
      className={cn(
        "zen-enter flex items-baseline gap-3 rounded-lg px-2 py-1 -mx-2 transition-colors",
        onClick && "cursor-pointer hover:bg-[color:var(--bg-hover)]/30",
        focused && "bg-[color:var(--accent)]/10 ring-1 ring-[color:var(--accent)]/40",
      )}
    >
      <span
        className="mt-2 h-1.5 w-1.5 shrink-0 self-start rounded-full"
        style={{ background: color }}
      />
      <div className="min-w-0 flex-1">
        <Markdown className="text-[length:var(--fs-emph)] leading-relaxed text-[color:var(--text)] [&_p]:my-0 [&_code]:text-[0.9em]">
          {text}
        </Markdown>
        {because && (
          <div className="mt-0.5 text-[length:var(--fs-label)] italic text-[color:var(--text-muted)]">
            because {because}
          </div>
        )}
      </div>
      {time != null && (
        <span className="shrink-0 self-start whitespace-nowrap text-[length:var(--fs-label)] text-[color:var(--text-muted)]">
          {time}
        </span>
      )}
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════════
// Zen Replay — auto-play a past session's sentences from the beginning.
// ═══════════════════════════════════════════════════════════════════

/** One beat of a replay. `kind` is already a union so a future
 *  `{ kind: "narration", text, ... }` variant can slot in once `/api/control`
 *  presents are persisted to the UI stream (backlog) — the reveal loop,
 *  progress bar, and pacing below don't need to change, only the row
 *  renderer gains a narration branch alongside this one. */
export interface ReplaySentenceItem {
  kind: "sentence";
  id: string;
  sessionId: string;
  eventId: string | null;
  text: string;
  because: string | null;
  color: string;
  /** Real wall-clock ISO timestamp from the pattern's `started_at`, when the
   *  API supplied one — verified present on turn.sentence patterns for a real
   *  session (started_at/ended_at + metadata.human.timestamp). Never faked:
   *  null falls back to a sequence position ("n / total") in the UI. */
  time: string | null;
  seq: number;
  total: number;
}
type ReplayItem = ReplaySentenceItem;

interface ReplayState {
  sessionId: string;
  items: ReplayItem[];
  /** Count of items revealed so far — `items.slice(0, revealed)` is on screen. */
  revealed: number;
  playing: boolean;
  speed: ReplaySpeed;
  finished: boolean;
}

/** turn.sentence patterns sorted into narrative order. Concurrent subagent
 *  timestamps can arrive slightly out of order; the turn number can't. */
function sortByTurn(patterns: readonly PatternView[]): PatternView[] {
  return [...patterns].sort(
    (a, b) => ((a.metadata?.turn as number) ?? 0) - ((b.metadata?.turn as number) ?? 0),
  );
}

function buildReplayItems(patterns: readonly PatternView[]): ReplayItem[] {
  const sorted = sortByTurn(patterns);
  return sorted.map((p, i) => {
    const { text, because } = zenSentence(p);
    const startedAt = p.metadata?.started_at;
    return {
      kind: "sentence",
      id: p.events[0] ?? `${p.session_id}-${i}`,
      sessionId: p.session_id,
      eventId: p.events[0] ?? null,
      text: shortenPaths(text),
      because: because ? shortenPaths(because) : null,
      color: STORY_COLOR[categorizeTurn(p)] ?? "var(--text-muted)",
      time: typeof startedAt === "string" && startedAt ? startedAt : null,
      seq: i + 1,
      total: sorted.length,
    };
  });
}

/** Base pacing: one sentence every ~1.8s at 1×. */
const REPLAY_BASE_MS = 1800;
const REPLAY_SPEEDS = [0.5, 1, 2, 4] as const;
type ReplaySpeed = (typeof REPLAY_SPEEDS)[number] | "real";

function speedLabel(speed: ReplaySpeed): string {
  return speed === "real" ? "real×60" : `${speed}×`;
}

/** Delay before revealing `items[nextIdx]`. "real×60" scales the actual gap
 *  between consecutive sentences' real timestamps by 1/60, clamped 0.6s–5s so
 *  a multi-hour silence doesn't stall the room and a rapid-fire burst doesn't
 *  flicker. Falls back to the flat pace when either timestamp is missing. */
function replayGapMs(items: readonly ReplayItem[], nextIdx: number, speed: ReplaySpeed): number {
  if (nextIdx <= 0) return speed === "real" ? 500 : Math.round(500 / speed);
  if (speed === "real") {
    const prev = items[nextIdx - 1]?.time;
    const cur = items[nextIdx]?.time;
    if (prev && cur) {
      const gap = new Date(cur).getTime() - new Date(prev).getTime();
      if (Number.isFinite(gap) && gap > 0) return Math.min(5000, Math.max(600, gap / 60));
    }
    return REPLAY_BASE_MS;
  }
  return REPLAY_BASE_MS / speed;
}

export function ZenView({
  records,
  patterns,
  onOpenEvent,
}: {
  records: readonly WireRecord[];
  patterns: readonly PatternView[];
  /** Click-through: a sentence is a PLACE — open its first event in Explore. */
  onOpenEvent?: (sessionId: string, eventId: string) => void;
}) {
  const { sessions } = useSessionsList();

  // People, most-recently-active first.
  const users = useMemo(() => {
    const latest = new Map<string, string>();
    for (const s of sessions as readonly SessionLike[]) {
      const u = s.user || "unknown";
      const le = s.last_event ?? "";
      if (le > (latest.get(u) ?? "")) latest.set(u, le);
    }
    return [...latest.entries()].sort((a, b) => (a[1] < b[1] ? 1 : -1)).map(([u]) => u);
  }, [sessions]);

  const [picked, setPicked] = useState<string | null>(null);
  const person = picked ?? users[0] ?? null;

  const personSessions = useMemo(
    () =>
      new Set(
        (sessions as readonly SessionLike[])
          .filter((s) => (s.user || "unknown") === person)
          .map((s) => s.session_id),
      ),
    [sessions, person],
  );

  // Replay session choice: this person's most recent ~5 sessions.
  const personSessionList = useMemo(
    () =>
      (sessions as readonly SessionLike[])
        .filter((s) => (s.user || "unknown") === person)
        .slice()
        .sort((a, b) => (b.last_event ?? "").localeCompare(a.last_event ?? ""))
        .slice(0, 5),
    [sessions, person],
  );

  // The story stream: turn sentences, in arrival order.
  const sentences = useMemo(
    () =>
      patterns
        .filter((p) => p.type === "turn.sentence" && personSessions.has(p.session_id))
        .map((p, i) => {
          const { text, because } = zenSentence(p);
          return {
            id: p.events[0] ?? `${p.session_id}-${i}`,
            sessionId: p.session_id,
            eventId: p.events[0] ?? null,
            text: shortenPaths(text),
            because: because ? shortenPaths(because) : null,
            color: STORY_COLOR[categorizeTurn(p)] ?? "var(--text-muted)",
          };
        })
        .slice(-ZEN_ROWS),
    [patterns, personSessions],
  );

  // Fallback for sessions with no sentences yet: one-line event bullets.
  const rows = useMemo(() => {
    if (sentences.length > 0) return [];
    return toTimelineRows(records)
      .filter((r) => r.category !== "turn" && personSessions.has(r.sessionId))
      .reverse()
      .slice(-ZEN_ROWS);
  }, [records, personSessions, sentences.length]);

  // Follow-live: pinned to the bottom until the reader scrolls up.
  const scrollRef = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);
  const count = sentences.length + rows.length;

  // ─── Replay state ───
  const [replay, setReplay] = useState<ReplayState | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const replayActive = replay !== null;

  useEffect(() => setPickerOpen(false), [person]);

  const startReplay = (sessionId: string | undefined) => {
    if (!sessionId) return;
    setPickerOpen(false);
    fetchSessionSentences(sessionId)
      .then((sessionPatterns) => {
        const items = buildReplayItems(sessionPatterns);
        setReplay({
          sessionId,
          items,
          revealed: 0,
          playing: true,
          speed: 1,
          finished: items.length === 0,
        });
      })
      .catch(() => setReplay(null));
  };

  const exitReplay = () => {
    setReplay(null);
    setFollow(true);
    requestAnimationFrame(() => {
      const el = scrollRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    });
  };

  // Reveal loop: schedules the next sentence at the current speed's pace.
  useEffect(() => {
    if (!replay || !replay.playing || replay.finished) return;
    if (replay.revealed >= replay.items.length) {
      setReplay((r) => (r ? { ...r, playing: false, finished: true } : r));
      return;
    }
    const delay = replayGapMs(replay.items, replay.revealed, replay.speed);
    const t = setTimeout(() => {
      setReplay((r) => {
        if (!r) return r;
        const revealed = r.revealed + 1;
        return { ...r, revealed, finished: revealed >= r.items.length };
      });
    }, delay);
    return () => clearTimeout(t);
  }, [replay]);

  // Synced smooth scroll: keep the newest replay line centered-low, in
  // unison with its zen-breathe entrance — "perfect unison", not a jump-cut.
  useEffect(() => {
    if (!replay || replay.revealed === 0) return;
    const item = replay.items[replay.revealed - 1];
    const container = scrollRef.current;
    if (!item || !container) return;
    const el = document.getElementById(`zen-replay-${item.id}`);
    if (!el) return;
    const containerRect = container.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();
    const elTop = elRect.top - containerRect.top + container.scrollTop;
    const target = Math.max(0, elTop - container.clientHeight * 0.62);
    container.scrollTo({ top: target, behavior: "smooth" });
    // Only the reveal count (and session identity, to reset on restart)
    // should retrigger this scroll — not every play/pause/speed change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [replay?.revealed, replay?.sessionId]);

  useEffect(() => {
    const el = scrollRef.current;
    if (el && follow && !replayActive) el.scrollTop = el.scrollHeight;
  }, [count, follow, replayActive]);
  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 80);
  };

  // Agent-in-UI seam: `toggle {target:"zen.focus", value:"<eventId>"}` lets a
  // narrator SPOTLIGHT a sentence — scroll to it and hold a glow on it, so
  // "the moment I just told you about" is a place on screen. Empty value clears.
  const [focusId, setFocusId] = useState<string | null>(null);
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type !== "toggle" || a.target !== "zen.focus") return;
      setFocusId(a.value || null);
    });
    return () => sub.unsubscribe();
  }, []);
  useEffect(() => {
    if (!focusId) return;
    setFollow(false);
    const el = document.getElementById(`zen-s-${focusId}`);
    if (el) el.scrollIntoView({ behavior: "smooth", block: "center" });
    const t = setTimeout(() => setFocusId(null), 12000);
    return () => clearTimeout(t);
  }, [focusId]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {/* The quietest possible header: replay controls, or just people. */}
      <div className="flex items-center gap-3 px-6 pt-4">
        {replay ? (
          replay.finished ? (
            <div className="flex w-full items-center justify-center gap-2 text-[length:var(--fs-label)] text-[color:var(--text-muted)]">
              <span>{replay.items.length === 0 ? "fin · nothing to replay" : "fin"}</span>
              {replay.items.length > 0 && (
                <>
                  <span>·</span>
                  <button
                    onClick={() => startReplay(replay.sessionId)}
                    className="underline underline-offset-4 transition-colors hover:text-[color:var(--text)]"
                  >
                    replay again
                  </button>
                  <span>/</span>
                </>
              )}
              <button
                onClick={exitReplay}
                className="underline underline-offset-4 transition-colors hover:text-[color:var(--text)]"
              >
                return to live
              </button>
            </div>
          ) : (
            <div className="flex w-full items-center gap-3 text-[length:var(--fs-label)] text-[color:var(--text-muted)]">
              <button
                onClick={() => setReplay((r) => (r ? { ...r, playing: !r.playing } : r))}
                className="transition-colors hover:text-[color:var(--text)]"
                title={replay.playing ? "Pause" : "Resume"}
              >
                {replay.playing ? "⏸" : "▶"}
              </button>
              <div className="flex items-center gap-1">
                {REPLAY_SPEEDS.map((s) => (
                  <button
                    key={s}
                    onClick={() => setReplay((r) => (r ? { ...r, speed: s } : r))}
                    className={cn(
                      "rounded px-1 transition-colors",
                      replay.speed === s
                        ? "text-[color:var(--text)] underline underline-offset-4"
                        : "hover:text-[color:var(--text)]",
                    )}
                  >
                    {speedLabel(s)}
                  </button>
                ))}
                {replay.items.some((i) => i.time) && (
                  <button
                    onClick={() => setReplay((r) => (r ? { ...r, speed: "real" } : r))}
                    className={cn(
                      "rounded px-1 transition-colors",
                      replay.speed === "real"
                        ? "text-[color:var(--text)] underline underline-offset-4"
                        : "hover:text-[color:var(--text)]",
                    )}
                    title="Pace by the session's real gaps between sentences, sped up 60×"
                  >
                    {speedLabel("real")}
                  </button>
                )}
              </div>
              <button
                onClick={exitReplay}
                className="transition-colors hover:text-[color:var(--text)]"
                title="Exit replay — return to live"
              >
                ⏹
              </button>
              <span className="ml-auto flex items-center gap-2">
                {replay.playing && (
                  <span className="pulse-live h-1.5 w-1.5 rounded-full" style={{ background: "var(--accent)" }} />
                )}
                {replay.revealed} / {replay.items.length}
              </span>
            </div>
          )
        ) : (
          <div className="flex w-full items-center justify-end gap-3">
            <div className="relative flex items-center gap-1">
              <button
                onClick={() => startReplay(personSessionList[0]?.session_id)}
                disabled={!personSessionList[0]}
                className="text-[length:var(--fs-label)] text-[color:var(--text-muted)] transition-colors hover:text-[color:var(--text)] disabled:opacity-40"
                title={
                  personSessionList[0]
                    ? `Replay ${person}'s most recent session`
                    : "No sessions to replay"
                }
              >
                ▶ replay
              </button>
              {personSessionList.length > 1 && (
                <button
                  onClick={() => setPickerOpen((o) => !o)}
                  className="text-[length:var(--fs-label)] text-[color:var(--text-muted)] transition-colors hover:text-[color:var(--text)]"
                  title="Choose a session to replay"
                >
                  ▾
                </button>
              )}
              {pickerOpen && (
                <>
                  <div className="fixed inset-0 z-10" onClick={() => setPickerOpen(false)} />
                  <div className="absolute right-0 top-full z-20 mt-1 w-56 rounded-lg border border-[color:var(--border)] bg-[color:var(--bg-surface)] py-1 shadow-card">
                    {personSessionList.map((s) => (
                      <button
                        key={s.session_id}
                        onClick={() => startReplay(s.session_id)}
                        className="block w-full truncate px-3 py-1 text-left text-[length:var(--fs-label)] text-[color:var(--text-muted)] transition-colors hover:bg-[color:var(--bg-hover)]/30 hover:text-[color:var(--text)]"
                      >
                        {s.label || s.session_id.slice(0, 8)}
                        {s.last_event && <span className="ml-1 opacity-70">· {relativeTime(s.last_event)}</span>}
                      </button>
                    ))}
                  </div>
                </>
              )}
            </div>
            {users.slice(0, 6).map((u) => (
              <button
                key={u}
                onClick={() => setPicked(u)}
                className={cn(
                  "text-[length:var(--fs-label)] transition-colors",
                  u === person
                    ? "text-[color:var(--text)] underline underline-offset-4"
                    : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]",
                )}
              >
                {u}
              </button>
            ))}
          </div>
        )}
      </div>

      {replay && (
        <div
          className="mx-6 mt-3 h-px shrink-0 bg-[color:var(--accent)] transition-[width] duration-300 ease-out"
          style={{
            opacity: 0.4,
            width: replay.items.length > 0 ? `${(replay.revealed / replay.items.length) * 100}%` : "100%",
          }}
        />
      )}

      <div ref={scrollRef} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto">
        {replay ? (
          replay.items.length === 0 ? (
            <EmptyState
              title="Nothing to replay"
              hint="This session doesn't have any story sentences yet."
            />
          ) : (
            <div className="mx-auto max-w-[62ch] space-y-4 px-6 py-10">
              {replay.items.slice(0, replay.revealed).map((item) => (
                <ZenSentenceRow
                  key={item.id}
                  id={`zen-replay-${item.id}`}
                  text={item.text}
                  because={item.because}
                  color={item.color}
                  time={item.time ? compactTime(item.time) : `${item.seq} / ${item.total}`}
                  onClick={
                    item.eventId && onOpenEvent
                      ? () => onOpenEvent(item.sessionId, item.eventId!)
                      : undefined
                  }
                />
              ))}
            </div>
          )
        ) : count === 0 ? (
          <EmptyState
            title="Stillness"
            hint={person ? `When ${person} acts, the story will appear here — one sentence at a time.` : "No activity yet."}
          />
        ) : (
          <div className="mx-auto max-w-[62ch] space-y-4 px-6 py-10">
            {sentences.map((s) => (
              <ZenSentenceRow
                key={s.id}
                id={`zen-s-${s.id}`}
                text={s.text}
                because={s.because}
                color={s.color}
                onClick={s.eventId && onOpenEvent ? () => onOpenEvent(s.sessionId, s.eventId!) : undefined}
                focused={focusId === s.id}
              />
            ))}
            {rows.map((r) => (
              <div key={r.id} className="zen-enter group flex items-baseline gap-3">
                <span
                  className="mt-1.5 h-1.5 w-1.5 shrink-0 self-start rounded-full"
                  style={{ background: KIND_COLOR[r.category] }}
                />
                <span className="min-w-0 flex-1 text-[length:var(--fs-emph)] leading-relaxed text-[color:var(--text)]">
                  {shortenPaths(r.summary)}
                </span>
                <span
                  className="shrink-0 text-[length:var(--fs-label)] text-[color:var(--text-muted)] opacity-0 transition-opacity group-hover:opacity-100"
                  title={fullTimestamp(r.timestamp)}
                >
                  {compactTime(r.timestamp)}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>

      {!follow && !replay && (
        <button
          onClick={() => {
            setFollow(true);
            const el = scrollRef.current;
            if (el) el.scrollTop = el.scrollHeight;
          }}
          className="absolute bottom-4 left-1/2 -translate-x-1/2 rounded-full border border-[color:var(--border)] bg-[color:var(--bg-surface)] px-3 py-1 text-[length:var(--fs-label)] text-[color:var(--text-muted)] shadow-card hover:text-[color:var(--text)]"
        >
          ↓ return to live
        </button>
      )}
    </div>
  );
}
