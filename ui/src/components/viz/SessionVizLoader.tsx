/** Fetches a session's records once and renders its visual summary:
 *  the activity ribbon (temporal shape) + the tool-call trace (durations).
 *  Used in the Overview drill-in where only a session id is in hand. */

import { useEffect, useState } from "react";
import type { WireRecord } from "@/types/wire-record";
import { SessionActivityRibbon } from "./SessionActivityRibbon";
import { TurnTraceView } from "./TurnTraceView";
import { TokenReport } from "./TokenReport";
import { SubagentsSection } from "./SubagentsSection";
import { SessionSummaryHeader } from "./SessionSummaryHeader";
import { SessionVizSkeleton } from "@/components/overview/OverviewSkeletons";
import { ConversationView } from "@/components/conversation/ConversationView";
import { cn } from "@/lib/cn";

/** The panel leads with the CONVERSATION (the story), with the tool trace a
 *  click away — "show me the conversation, not the numbers." */
type Lens = "conversation" | "trace";

export function SessionVizLoader({ sessionId, onOpenSubagent }: { sessionId: string; onOpenSubagent?: (id: string) => void }) {
  const [records, setRecords] = useState<WireRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [lens, setLens] = useState<Lens>("conversation");

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setRecords([]);
    fetch(`/api/sessions/${sessionId}/records`)
      .then((r) => r.json())
      .then((data: WireRecord[]) => {
        if (!cancelled) {
          setRecords(Array.isArray(data) ? data : []);
          setLoading(false);
        }
      })
      .catch(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [sessionId]);

  if (loading) return <SessionVizSkeleton />;

  return (
    <div>
      <div className="border-b border-[#2f3348] bg-[#24283b]">
        <SessionSummaryHeader records={records} />
      </div>
      <SessionActivityRibbon records={records} />
      <div className="mt-1 border-t border-[#2f3348] pt-1">
        {/* Conversation-first: the transcript leads, the tool trace is a click away. */}
        <div className="flex items-center gap-1 px-3 pt-1">
          {([
            { key: "conversation", label: "Conversation" },
            { key: "trace", label: "Tool trace" },
          ] as const).map((t) => (
            <button
              key={t.key}
              onClick={() => setLens(t.key)}
              aria-pressed={lens === t.key}
              className={cn(
                "rounded px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide transition-colors",
                lens === t.key ? "bg-[#7aa2f7] text-[#1a1b26]" : "text-[#565f89] hover:text-[#c0caf5]",
              )}
            >
              {t.label}
            </button>
          ))}
        </div>
        {lens === "conversation" ? (
          <div className="flex h-[58vh] max-h-[560px] min-h-[240px] flex-col">
            <ConversationView sessionId={sessionId} />
          </div>
        ) : (
          <TurnTraceView records={records} />
        )}
      </div>
      <div className="mt-1 border-t border-[#2f3348] pt-1">
        <div className="px-3 pt-1 text-[10px] font-medium uppercase tracking-wide text-[#565f89]">Tokens</div>
        <TokenReport records={records} />
      </div>
      <div className="mt-1 border-t border-[#2f3348] pt-1">
        <SubagentsSection records={records} onOpen={onOpenSubagent} />
      </div>
    </div>
  );
}
