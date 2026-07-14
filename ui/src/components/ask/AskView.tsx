/** AskView — "ask the data": pick a question, get an answer computed live from
 *  your session history. Pure read (Pattern 3, sovereignty-safe — no writes, no
 *  LLM). Answers drill through to Explore. Free-text natural language would need
 *  an LLM proxy (a later step, still read-only). */

import { useMemo, useState, useEffect } from "react";
import { controlActions$ } from "@/streams/control";
import type { HashRoute } from "@/lib/hash-route";
import { useSessionsList } from "@/hooks/use-sessions-list";
import { isSubagentSession } from "@/lib/subagents";
import { answer, QUESTIONS } from "@/lib/insights";
import { sessionColor } from "@/lib/session-colors";
import { cn } from "@/lib/cn";

export function AskView({ onNavigate }: { onNavigate: (route: HashRoute) => void }) {
  const { sessions, loading } = useSessionsList();
  const universe = useMemo(() => sessions.filter((s) => !isSubagentSession(s.session_id)), [sessions]);
  const nowMs = useMemo(() => Date.now(), []);
  const [qid, setQid] = useState<string>("latest");
  const ans = useMemo(() => answer(universe, qid, nowMs), [universe, qid, nowMs]);

  // Agent-in-UI: apply `ask.question` toggle intents (component-local). Sink only.
  useEffect(() => {
    const sub = controlActions$().subscribe((a) => {
      if (a.type === "toggle" && a.target === "ask.question" && QUESTIONS.some((q) => q.id === a.value)) {
        setQid(a.value);
      }
    });
    return () => sub.unsubscribe();
  }, []);

  return (
    <div className="flex min-h-0 flex-1 bg-[color:var(--bg)] text-[color:var(--text)]" data-testid="ask-view">
      {/* question list */}
      <div className="flex w-72 shrink-0 flex-col border-r border-[color:var(--bg-hover)] p-3">
        <div className="mb-1 text-[13px] font-semibold text-[color:var(--text)]">Ask your fleet</div>
        <div className="mb-3 text-[10px] text-[color:var(--text-muted)]">Answered live from your {universe.length.toLocaleString()} sessions — read-only, nothing leaves your machine.</div>
        <div className="flex flex-col gap-1">
          {QUESTIONS.map((q) => (
            <button
              key={q.id}
              onClick={() => setQid(q.id)}
              className={cn("rounded px-2.5 py-1.5 text-left text-[12px] transition-colors", qid === q.id ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text-bright)] hover:bg-[color:var(--bg-surface)]")}
            >
              {q.q}
            </button>
          ))}
        </div>
        <div className="mt-auto pt-4 text-[10px] leading-relaxed text-[color:var(--text-muted)]">
          Free-text questions are coming — they'll route through your read-only analytics tools, never writing to or steering your agents.
        </div>
      </div>

      {/* answer */}
      <div className="min-w-0 flex-1 overflow-y-auto p-5">
        {loading ? (
          <div className="text-[12px] text-[color:var(--text-muted)]">Loading…</div>
        ) : (
          <div className="mx-auto max-w-2xl">
            <div className="mb-1 text-[18px] font-semibold text-[color:var(--text)]">{ans.title}</div>
            {ans.note && <div className="mb-4 text-[12px] text-[color:var(--text-muted)]">{ans.note}</div>}
            {ans.items.length === 0 ? (
              <div className="text-[12px] text-[color:var(--text-muted)]">Nothing to show.</div>
            ) : (
              <div className="flex flex-col divide-y divide-[color:var(--bg-hover)]/60">
                {ans.items.map((it, i) => (
                  <button
                    key={i}
                    data-ask-item
                    disabled={!it.sessionId}
                    onClick={it.sessionId ? () => onNavigate({ view: "explore", sessionId: it.sessionId }) : undefined}
                    className={cn("flex items-center gap-3 py-2 text-left", it.sessionId ? "hover:bg-[color:var(--bg-surface)] cursor-pointer" : "cursor-default")}
                  >
                    <span className="w-5 shrink-0 text-right text-[11px] tabular-nums text-[color:var(--text-muted)]">{i + 1}</span>
                    {it.sessionId && <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: sessionColor(it.sessionId) }} />}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[13px] text-[color:var(--text)]">{it.label}</span>
                      {it.sub && <span className="block truncate text-[10px] text-[color:var(--text-muted)]">{it.sub}</span>}
                    </span>
                    <span className="shrink-0 text-[12px] tabular-nums text-[color:var(--accent)]">{it.value}</span>
                    {it.sessionId && <span className="shrink-0 text-[color:var(--text-muted)]">→</span>}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
