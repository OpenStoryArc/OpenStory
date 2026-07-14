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
interface SentenceSub {
  role: string;
  verb: string;
  object: string;
  tool_calls: number;
}
interface SentenceAnatomy {
  verb: string;
  object: string;
  /** Flat fallback (ghost lines, plain contexts). */
  text: string;
  because: string | null;
  subs: SentenceSub[];
  predicate: string | null;
}

function zenSentence(p: PatternView): SentenceAnatomy {
  const m = p.metadata ?? {};
  const verb = typeof m.verb === "string" ? m.verb.trim() : "";
  const object = typeof m.object === "string" ? m.object.trim() : "";
  const text = [verb, object].filter(Boolean).join(" ").trim() || p.label || "…";
  let because: string | null = null;
  if (typeof m.adverbial === "string" && m.adverbial.trim()) {
    const raw = m.adverbial.trim().replace(/^"|"$/g, "").replace(/\s+/g, " ");
    because = raw.slice(0, 420).trim() || null;
    if (because && raw.length > 420) because += "…";
  }
  const subs = Array.isArray(m.subordinates)
    ? (m.subordinates as SentenceSub[]).filter((x) => x && (x.verb || x.object))
    : [];
  const predicate = typeof m.predicate === "string" && m.predicate.trim() ? m.predicate.trim() : null;
  return { verb: verb || (text === object ? "" : text), object, text, because, subs, predicate };
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
  anatomy,
  color,
  time,
  onClick,
  focused,
}: {
  id: string;
  anatomy: SentenceAnatomy;
  color: string;
  /** Replay-only: a compactTime or "n / total" label. */
  time?: string | null;
  onClick?: () => void;
  focused?: boolean;
}) {
  const { verb, object, text, because, subs, predicate } = anatomy;
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
        {/* The sentence spine: VERB (category-colored, weighted) + object.
            Same grammar as the Story tab's diagram, set as calm prose. */}
        {verb && object ? (
          <div className="text-[length:var(--fs-emph)] leading-relaxed">
            <span className="font-semibold" style={{ color }}>
              {verb}
            </span>{" "}
            <Markdown className="inline text-[color:var(--text)] [&_p]:my-0 [&_p]:inline [&_code]:text-[0.9em]">
              {object}
            </Markdown>
          </div>
        ) : (
          <Markdown className="text-[length:var(--fs-emph)] leading-relaxed text-[color:var(--text)] [&_p]:my-0 [&_code]:text-[0.9em]">
            {text}
          </Markdown>
        )}
        {/* Subordinate clauses — the tree branches, as quiet indents. */}
        {subs.slice(0, 3).map((sub, i) => (
          <div
            key={i}
            className="ml-0.5 mt-0.5 border-l border-[color:var(--divider)] pl-2.5 text-[length:var(--fs-body)] leading-relaxed text-[color:var(--text-muted)]"
          >
            <span className="text-[color:var(--text-bright)]">{sub.verb}</span>{" "}
            {shortenPaths(sub.object)}
            {sub.tool_calls > 0 && <span className="opacity-70"> ({sub.tool_calls})</span>}
          </div>
        ))}
        {subs.length > 3 && (
          <div className="ml-0.5 border-l border-[color:var(--divider)] pl-2.5 text-[length:var(--fs-label)] text-[color:var(--text-muted)] opacity-70">
            +{subs.length - 3} more
          </div>
        )}
        {/* The prompt that caused it — the human voice. Quietly distinct
            (Max: the wash version overdid it): a firm category-colored quote
            bar, ink one step brighter than muted, no background. */}
        {because && (
          <div
            className="ml-0.5 mt-1 border-l-2 pl-2.5 text-[length:var(--fs-body)] leading-relaxed text-[color:var(--text-bright)]"
            style={{ borderColor: `color-mix(in oklab, ${color} 60%, transparent)` }}
          >
            “{because}”
          </div>
        )}
        {predicate && (because || subs.length > 0) && (
          <div className="mt-0.5 text-[length:var(--fs-label)] text-[color:var(--green)] opacity-80">
            → {predicate}
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
  anatomy: SentenceAnatomy;
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
    const a = zenSentence(p);
    const startedAt = p.metadata?.started_at;
    return {
      kind: "sentence" as const,
      id: p.events[0] ?? `${p.session_id}-${i}`,
      sessionId: p.session_id,
      eventId: p.events[0] ?? null,
      anatomy: {
        ...a,
        object: shortenPaths(a.object),
        text: shortenPaths(a.text),
        because: a.because ? shortenPaths(a.because) : null,
      },
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
          const a = zenSentence(p);
          const anatomy = {
            ...a,
            object: shortenPaths(a.object),
            text: shortenPaths(a.text),
            because: a.because ? shortenPaths(a.because) : null,
          };
          return {
            id: p.events[0] ?? `${p.session_id}-${i}`,
            sessionId: p.session_id,
            eventId: p.events[0] ?? null,
            text: anatomy.text,
            anatomy,
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

  // ─── Graceful delivery + back-pressure (live mode) ───
  // Arrivals queue instead of shoving in: one sentence is RELEASED every
  // ~1.4s (÷ delivery speed), history appears instantly on load/person
  // switch, and the un-released remainder is made visible as ghost lines +
  // a pressure chip. Max's spec: see the burst coming, never be buried by it.
  const [shownCount, setShownCount] = useState(0);
  const [deliverySpeed, setDeliverySpeed] = useState<1 | 2 | 4>(1);
  const [catchUp, setCatchUp] = useState(false);
  const personRef = useRef<string | null>(null);
  const bootRef = useRef(true);
  useEffect(() => {
    // First load or person switch: snap to now — pacing is for LIVE arrivals,
    // not a slow replay of history.
    if (bootRef.current || personRef.current !== person) {
      bootRef.current = false;
      personRef.current = person;
      setShownCount(sentences.length);
      setCatchUp(false);
      return;
    }
    if (sentences.length < shownCount) {
      setShownCount(sentences.length); // cap rotation / shrink — resync
      return;
    }
    if (sentences.length === shownCount) {
      if (catchUp) setCatchUp(false); // drained — chip fades, speed resumes
      return;
    }
    const gap = catchUp ? 90 : Math.round(1400 / deliverySpeed);
    const t = setTimeout(
      () => setShownCount((c) => Math.min(c + 1, sentences.length)),
      gap,
    );
    return () => clearTimeout(t);
  }, [sentences.length, shownCount, person, deliverySpeed, catchUp]);

  const visibleSentences = sentences.slice(0, shownCount);
  const ghosts = sentences.slice(shownCount, shownCount + 2);
  const queued = Math.max(0, sentences.length - shownCount);
  // Pressure reads at a glance: calm cyan → amber → pulsing red as depth grows.
  const pressureColor =
    queued > 10 ? "var(--red)" : queued > 3 ? "var(--orange)" : "var(--cyan)";
  const pressureSize = queued > 10 ? "h-2.5 w-2.5" : queued > 3 ? "h-2 w-2" : "h-1.5 w-1.5";

  // Follow-live: pinned to the bottom until the reader scrolls up.
  const scrollRef = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);
  const count = shownCount + rows.length;

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

  // ─── Guided mode: "ask, and I answer by arranging the room." ───
  // A narrator CONDUCTS a curated sequence through the seam, one sentence at
  // a time, pacing it with their own voice:
  //   toggle zen.guide       value='{"title":"..."}'          → open the room
  //   toggle zen.guide.item  value='{"sessionId","eventId"}'  → breathe one in
  //   toggle zen.guide.done  value=""                          → fin
  //   toggle zen.guide       value=""                          → back to live
  // Read-only on the record; every arrival is a door to its source.
  interface GuidedItem {
    id: string;
    sessionId: string;
    eventId: string | null;
    anatomy: SentenceAnatomy;
    color: string;
    time: string | null;
  }
  const [guided, setGuided] = useState<{ title: string; items: GuidedItem[]; done: boolean } | null>(null);
  const guideCache = useRef<Map<string, PatternView[]>>(new Map());
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type !== "toggle") return;
      if (a.target === "zen.guide") {
        if (!a.value) {
          setGuided(null);
          return;
        }
        try {
          const cfg = JSON.parse(a.value) as { title?: string };
          setGuided({ title: cfg.title || "a guided answer", items: [], done: false });
        } catch {
          /* malformed guide config — ignore */
        }
      } else if (a.target === "zen.guide.done") {
        setGuided((g) => (g ? { ...g, done: true } : g));
      } else if (a.target === "zen.guide.item") {
        try {
          const ref = JSON.parse(a.value) as { sessionId?: string; eventId?: string; turn?: number };
          if (!ref.sessionId || (!ref.eventId && ref.turn == null)) return;
          const sid = ref.sessionId;
          const eid = ref.eventId ?? null;
          const cached = guideCache.current.get(sid);
          const withPatterns = (pats: PatternView[]) => {
            guideCache.current.set(sid, pats);
            const p = eid
              ? pats.find((x) => x.events.includes(eid))
              : pats.find((x) => (x.metadata?.turn as number | undefined) === ref.turn);
            if (!p) return;
            const a2 = zenSentence(p);
            const realEid = p.events[0] ?? eid;
            const item: GuidedItem = {
              id: realEid ?? `${sid}-t${ref.turn}`,
              sessionId: sid,
              eventId: realEid,
              anatomy: {
                ...a2,
                object: shortenPaths(a2.object),
                text: shortenPaths(a2.text),
                because: a2.because ? shortenPaths(a2.because) : null,
              },
              color: STORY_COLOR[categorizeTurn(p)] ?? "var(--text-muted)",
              time: typeof p.metadata?.started_at === "string" ? (p.metadata.started_at as string) : null,
            };
            setGuided((g) => {
              if (!g || g.items.some((x) => x.id === item.id)) return g;
              return { ...g, items: [...g.items, item] };
            });
          };
          if (cached) withPatterns(cached);
          else fetchSessionSentences(sid).then(withPatterns).catch(() => {});
        } catch {
          /* malformed item ref — ignore */
        }
      }
    });
    return () => sub.unsubscribe();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Guided scroll: newest conducted sentence settles centered-low, in unison.
  useEffect(() => {
    if (!guided || guided.items.length === 0) return;
    const last = guided.items[guided.items.length - 1];
    const container = scrollRef.current;
    if (!last || !container) return;
    const el = document.getElementById(`zen-guide-${last.id}`);
    if (!el) return;
    const containerRect = container.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();
    const elTop = elRect.top - containerRect.top + container.scrollTop;
    container.scrollTo({ top: Math.max(0, elTop - container.clientHeight * 0.62), behavior: "smooth" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [guided?.items.length]);

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
        {guided ? (
          <div className="mx-auto max-w-[62ch] space-y-4 px-6 py-10">
            <div className="flex items-center gap-2 text-[length:var(--fs-label)] uppercase tracking-wide text-[color:var(--text-muted)]">
              <span className="h-1 w-1 rounded-full bg-[color:var(--accent)]" />
              {guided.title}
            </div>
            {guided.items.map((item) => (
              <ZenSentenceRow
                key={item.id}
                id={`zen-guide-${item.id}`}
                anatomy={item.anatomy}
                color={item.color}
                time={item.time ? compactTime(item.time) : null}
                onClick={
                  item.eventId && onOpenEvent
                    ? () => onOpenEvent(item.sessionId, item.eventId!)
                    : undefined
                }
              />
            ))}
            {guided.items.length === 0 && !guided.done && (
              <div className="text-[length:var(--fs-body)] italic text-[color:var(--text-muted)]">
                listening…
              </div>
            )}
            {guided.done && (
              <div className="flex items-center gap-2 pt-2 text-[length:var(--fs-label)] text-[color:var(--text-muted)]">
                <span>fin</span>
                <span>·</span>
                <button
                  onClick={() => setGuided(null)}
                  className="underline underline-offset-4 transition-colors hover:text-[color:var(--text)]"
                >
                  return to live
                </button>
              </div>
            )}
          </div>
        ) : replay ? (
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
                  anatomy={item.anatomy}
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
            {visibleSentences.map((s) => (
              <ZenSentenceRow
                key={s.id}
                id={`zen-s-${s.id}`}
                anatomy={s.anatomy}
                color={s.color}
                onClick={s.eventId && onOpenEvent ? () => onOpenEvent(s.sessionId, s.eventId!) : undefined}
                focused={focusId === s.id}
              />
            ))}
            {/* Ghost lines: the next queued sentences, fading into the fog —
                you can SEE more arriving before it lands. */}
            {ghosts.map((g, gi) => (
              <div
                key={`ghost-${g.id}`}
                aria-hidden
                className={cn(
                  "pointer-events-none flex select-none items-baseline gap-3 px-2 -mx-2",
                  gi === 0 ? "opacity-40 blur-[1px]" : "opacity-20 blur-[2px]",
                )}
              >
                <span className="mt-2 h-1.5 w-1.5 shrink-0 self-start rounded-full bg-[color:var(--text-muted)]" />
                <span className="min-w-0 flex-1 text-[length:var(--fs-emph)] leading-relaxed text-[color:var(--text-muted)]">
                  {g.text}
                </span>
              </div>
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

      {/* Back-pressure chip: how much is still arriving, and the throttle. */}
      {!replay && queued > 0 && (
        <div className="chat-enter absolute bottom-4 left-1/2 flex -translate-x-1/2 items-center gap-2.5 rounded-full border border-[color:var(--divider)] bg-[color:var(--bg-surface)]/95 px-3.5 py-1.5 shadow-card backdrop-blur-sm">
          <span
            className={cn("shrink-0 rounded-full transition-all", pressureSize, queued > 10 && "pulse-live")}
            style={{ background: pressureColor }}
          />
          <span className="text-[length:var(--fs-label)] tabular-nums text-[color:var(--text-bright)]">
            {queued} arriving
          </span>
          <span className="h-3 w-px bg-[color:var(--divider)]" />
          {([1, 2, 4] as const).map((s) => (
            <button
              key={s}
              onClick={() => setDeliverySpeed(s)}
              className={cn(
                "text-[length:var(--fs-label)] transition-colors",
                deliverySpeed === s && !catchUp
                  ? "text-[color:var(--text)] underline underline-offset-4"
                  : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]",
              )}
              title={`Deliver one sentence every ${(1.4 / s).toFixed(1)}s`}
            >
              {s}×
            </button>
          ))}
          <button
            onClick={() => {
              setCatchUp(true);
              setFollow(true);
            }}
            className="text-[length:var(--fs-label)] text-[color:var(--accent)] transition-colors hover:underline"
            title="Fast-drain the queue to now"
          >
            ⏩ catch up
          </button>
        </div>
      )}

      {!follow && !replay && queued === 0 && (
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
