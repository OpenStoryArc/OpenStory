/** ZenView — the calm room.
 *
 *  One quiet bullet per event: a kind-colored dot and a single line of text
 *  on a blank canvas. No cards, no chrome, no numbers competing for the eye.
 *  Watches ONE person at a time (picker top-right, defaults to whoever was
 *  active most recently). New lines breathe in at the bottom; scroll up to
 *  read history, and a "return to live" whisper brings you back.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type { WireRecord } from "@/types/wire-record";
import { toTimelineRows, type TimelineCategory } from "@/lib/timeline";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { compactTime, fullTimestamp } from "@/lib/time";
import { EmptyState } from "@/components/ui/EmptyState";
import { cn } from "@/lib/cn";

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

/** How much history the room holds — enough to scroll, never a wall. */
const ZEN_ROWS = 400;

interface SessionLike {
  session_id: string;
  user?: string | null;
  last_event?: string | null;
}

export function ZenView({ records }: { records: readonly WireRecord[] }) {
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

  // toTimelineRows returns newest-first (the Live feed's order); the zen room
  // reads top-to-bottom like a page, so reverse into chronological order.
  const rows = useMemo(() => {
    const all = toTimelineRows(records)
      .filter((r) => r.category !== "turn" && personSessions.has(r.sessionId))
      .reverse();
    return all.slice(-ZEN_ROWS);
  }, [records, personSessions]);

  // Follow-live: pinned to the bottom until the reader scrolls up.
  const scrollRef = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);
  useEffect(() => {
    const el = scrollRef.current;
    if (el && follow) el.scrollTop = el.scrollHeight;
  }, [rows.length, follow]);
  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 80);
  };

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
        {rows.length === 0 ? (
          <EmptyState
            title="Stillness"
            hint={person ? `When ${person} acts, it will appear here — one line at a time.` : "No activity yet."}
          />
        ) : (
          <div className="mx-auto max-w-[62ch] space-y-3 px-6 py-10">
            {rows.map((r) => (
              <div key={r.id} className="zen-enter group flex items-baseline gap-3">
                <span
                  className="mt-1.5 h-1.5 w-1.5 shrink-0 self-start rounded-full"
                  style={{ background: KIND_COLOR[r.category] }}
                />
                <span className="min-w-0 flex-1 text-[length:var(--fs-emph)] leading-relaxed text-[color:var(--text)]">
                  {r.summary}
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
