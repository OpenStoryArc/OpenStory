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
    <div className="flex min-h-0 flex-1 bg-[#1a1b26] text-[#c0caf5]" data-testid="ask-view">
      {/* question list */}
      <div className="flex w-72 shrink-0 flex-col border-r border-[#2f3348] p-3">
        <div className="mb-1 text-[13px] font-semibold text-[#c0caf5]">Ask your fleet</div>
        <div className="mb-3 text-[10px] text-[#565f89]">Answered live from your {universe.length.toLocaleString()} sessions — read-only, nothing leaves your machine.</div>
        <div className="flex flex-col gap-1">
          {QUESTIONS.map((q) => (
            <button
              key={q.id}
              onClick={() => setQid(q.id)}
              className={cn("rounded px-2.5 py-1.5 text-left text-[12px] transition-colors", qid === q.id ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#a9b1d6] hover:bg-[#24283b]")}
            >
              {q.q}
            </button>
          ))}
        </div>
        <div className="mt-auto pt-4 text-[10px] leading-relaxed text-[#565f89]">
          Free-text questions are coming — they'll route through your read-only analytics tools, never writing to or steering your agents.
        </div>
      </div>

      {/* answer */}
      <div className="min-w-0 flex-1 overflow-y-auto p-5">
        {loading ? (
          <div className="text-[12px] text-[#565f89]">Loading…</div>
        ) : (
          <div className="mx-auto max-w-2xl">
            <div className="mb-1 text-[18px] font-semibold text-[#c0caf5]">{ans.title}</div>
            {ans.note && <div className="mb-4 text-[12px] text-[#565f89]">{ans.note}</div>}
            {ans.items.length === 0 ? (
              <div className="text-[12px] text-[#565f89]">Nothing to show.</div>
            ) : (
              <div className="flex flex-col divide-y divide-[#2f3348]/60">
                {ans.items.map((it, i) => (
                  <button
                    key={i}
                    data-ask-item
                    disabled={!it.sessionId}
                    onClick={it.sessionId ? () => onNavigate({ view: "explore", sessionId: it.sessionId }) : undefined}
                    className={cn("flex items-center gap-3 py-2 text-left", it.sessionId ? "hover:bg-[#24283b] cursor-pointer" : "cursor-default")}
                  >
                    <span className="w-5 shrink-0 text-right text-[11px] tabular-nums text-[#565f89]">{i + 1}</span>
                    {it.sessionId && <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: sessionColor(it.sessionId) }} />}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[13px] text-[#c0caf5]">{it.label}</span>
                      {it.sub && <span className="block truncate text-[10px] text-[#565f89]">{it.sub}</span>}
                    </span>
                    <span className="shrink-0 text-[12px] tabular-nums text-[#7aa2f7]">{it.value}</span>
                    {it.sessionId && <span className="shrink-0 text-[#565f89]">→</span>}
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
