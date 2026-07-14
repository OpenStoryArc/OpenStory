/** Renders a session's visual summary from the shared record cache:
 *  the activity ribbon (temporal shape) + the tool-call trace (durations).
 *  Used in the Overview drill-in where only a session id is in hand. */

import { useState } from "react";
import { useSessionRecords } from "@/hooks/use-session-records";
import { SessionActivityRibbon } from "./SessionActivityRibbon";
import { TurnTraceView } from "./TurnTraceView";
import { TokenReport } from "./TokenReport";
import { SubagentsSection } from "./SubagentsSection";
import { SessionSummaryHeader } from "./SessionSummaryHeader";
import { SessionVizSkeleton } from "@/components/ui/skeletons";
import { ConversationView } from "@/components/conversation/ConversationView";
import { cn } from "@/lib/cn";

/** The panel leads with the CONVERSATION (the story), with the tool trace a
 *  click away — "show me the conversation, not the numbers." */
type Lens = "conversation" | "trace";

export function SessionVizLoader({ sessionId, onOpenSubagent, onOpenStory }: { sessionId: string; onOpenSubagent?: (id: string) => void; onOpenStory?: () => void }) {
  const { records, loading } = useSessionRecords(sessionId);
  const [lens, setLens] = useState<Lens>("conversation");

  if (loading) return <SessionVizSkeleton />;

  return (
    <div>
      {/* Header: the session summary card + an optional jump to its Story. */}
      <div className="flex items-start justify-between gap-2 border-b border-[color:var(--bg-hover)] bg-[color:var(--bg-surface)]">
        <div className="min-w-0 flex-1"><SessionSummaryHeader records={records} /></div>
        {onOpenStory && (
          <button
            onClick={onOpenStory}
            className="shrink-0 whitespace-nowrap px-3 py-2 text-[11px] text-[color:var(--purple)] hover:bg-[color:var(--bg-hover)]"
            title="Open this session's Story"
          >
            Story →
          </button>
        )}
      </div>
      {/* Key data on top: the token summary, before the conversation. */}
      <div className="border-b border-[color:var(--bg-hover)]">
        <div className="px-3 pt-1 text-[10px] font-medium uppercase tracking-wide text-[color:var(--text-muted)]">Tokens</div>
        <TokenReport records={records} />
      </div>
      <SessionActivityRibbon records={records} />
      <div className="mt-1 border-t border-[color:var(--bg-hover)] pt-1">
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
                lens === t.key ? "bg-[color:var(--accent)] text-[color:var(--bg)]" : "text-[color:var(--text-muted)] hover:text-[color:var(--text)]",
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
      <div className="mt-1 border-t border-[color:var(--bg-hover)] pt-1">
        <SubagentsSection records={records} onOpen={onOpenSubagent} />
      </div>
    </div>
  );
}
