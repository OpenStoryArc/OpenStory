/** ZenView — the calm room.
 *
 *  The story, one sentence at a time: as turns complete, their narrative
 *  sentences (verb + object, with an optional "because") breathe in at the
 *  bottom of a blank canvas. Markdown renders inline; long absolute paths are
 *  shortened to their last two segments (a full /Users/... path is not zen).
 *  Watches ONE person at a time. Falls back to one-line event bullets for
 *  sessions that have no sentences yet.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type { WireRecord, PatternView } from "@/types/wire-record";
import { toTimelineRows, type TimelineCategory } from "@/lib/timeline";
import { categorizeTurn, type StoryCategory } from "@/lib/story";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { compactTime, fullTimestamp } from "@/lib/time";
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
  useEffect(() => {
    const el = scrollRef.current;
    if (el && follow) el.scrollTop = el.scrollHeight;
  }, [count, follow]);
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
      {/* The quietest possible header: just people. */}
      <div className="flex items-center justify-end gap-3 px-6 pt-4">
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

      <div ref={scrollRef} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto">
        {count === 0 ? (
          <EmptyState
            title="Stillness"
            hint={person ? `When ${person} acts, the story will appear here — one sentence at a time.` : "No activity yet."}
          />
        ) : (
          <div className="mx-auto max-w-[62ch] space-y-4 px-6 py-10">
            {sentences.map((s) => (
              <div
                key={s.id}
                id={`zen-s-${s.id}`}
                onClick={s.eventId && onOpenEvent ? () => onOpenEvent(s.sessionId, s.eventId!) : undefined}
                title={s.eventId && onOpenEvent ? "Open this moment in Explore" : undefined}
                className={cn(
                  "zen-enter flex items-baseline gap-3 rounded-lg px-2 py-1 -mx-2 transition-colors",
                  s.eventId && onOpenEvent && "cursor-pointer hover:bg-[color:var(--bg-hover)]/30",
                  focusId === s.id && "bg-[color:var(--accent)]/10 ring-1 ring-[color:var(--accent)]/40",
                )}
              >
                <span
                  className="mt-2 h-1.5 w-1.5 shrink-0 self-start rounded-full"
                  style={{ background: s.color }}
                />
                <div className="min-w-0 flex-1">
                  <Markdown className="text-[length:var(--fs-emph)] leading-relaxed text-[color:var(--text)] [&_p]:my-0 [&_code]:text-[0.9em]">
                    {s.text}
                  </Markdown>
                  {s.because && (
                    <div className="mt-0.5 text-[length:var(--fs-label)] italic text-[color:var(--text-muted)]">
                      because {s.because}
                    </div>
                  )}
                </div>
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

      {!follow && (
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
