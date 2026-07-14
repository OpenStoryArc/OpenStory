/** Renders a session's visual summary from the shared record cache:
 *  the activity ribbon (temporal shape) + the tool-call trace (durations).
 *  Used in the Overview drill-in where only a session id is in hand. */

import { useMemo, useState } from "react";
import { useSessionRecords } from "@/hooks/use-session-records";
import { SessionActivityRibbon } from "./SessionActivityRibbon";
import { TurnTraceView } from "./TurnTraceView";
import { TokenReport } from "./TokenReport";
import { SubagentsSection } from "./SubagentsSection";
import { SessionSummaryHeader } from "./SessionSummaryHeader";
import { SessionVizSkeleton } from "@/components/ui/skeletons";
import { ConversationView } from "@/components/conversation/ConversationView";
import { SessionDetailPanel } from "@/components/session/SessionDetailPanel";
import { extractSubagents } from "@/lib/subagents";
import { cn } from "@/lib/cn";

/** The panel leads with the CONVERSATION (the story). Everything else —
 *  tool trace, subagents, the detail wall (synopsis / journey / files) —
 *  is a click away at the TOP, not a scroll below the transcript. */
type Lens = "conversation" | "trace" | "subagents" | "details";

export function SessionVizLoader({ sessionId, onOpenSubagent, onOpenStory }: { sessionId: string; onOpenSubagent?: (id: string) => void; onOpenStory?: () => void }) {
  const { records, loading } = useSessionRecords(sessionId);
  const [lens, setLens] = useState<Lens>("conversation");
  // Lazy-mount-once: a lens mounts on first visit, then stays mounted (CSS
  // hidden) so returning to it is instant — Details' 4 fetches and the
  // conversation's scroll position survive tab switches.
  const [visited, setVisited] = useState<ReadonlySet<Lens>>(() => new Set<Lens>(["conversation"]));
  const openLens = (k: Lens) => {
    setLens(k);
    setVisited((prev) => (prev.has(k) ? prev : new Set(prev).add(k)));
  };
  const subagentCount = useMemo(() => extractSubagents(records).length, [records]);

  if (loading) return <SessionVizSkeleton />;

  return (
    <div className="flex min-h-0 grow flex-col">
      {/* Header: the session summary card + an optional jump to its Story. */}
      <div className="flex items-start justify-between gap-2 border-b border-[color:var(--bg-hover)] bg-[color:var(--bg-surface)]">
        <div className="min-w-0 flex-1"><SessionSummaryHeader records={records} /></div>
        {onOpenStory && (
          <button
            onClick={onOpenStory}
            className="shrink-0 whitespace-nowrap px-3 py-2 text-[11px] text-[color:var(--accent)] hover:bg-[color:var(--bg-hover)]"
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
      <div className="mt-1 flex min-h-0 grow flex-col border-t border-[color:var(--bg-hover)] pt-1">
        {/* Conversation-first: the transcript leads; trace, subagents, and the
            detail wall are lens tabs at the top — no scroll-past-everything. */}
        <div className="flex items-center gap-1 px-3 pt-1">
          {([
            { key: "conversation", label: "Conversation" },
            { key: "trace", label: "Tool trace" },
            ...(subagentCount > 0
              ? ([{ key: "subagents", label: `Subagents · ${subagentCount}` }] as const)
              : []),
            { key: "details", label: "Details" },
          ] as { key: Lens; label: string }[]).map((t) => (
            <button
              key={t.key}
              onClick={() => openLens(t.key)}
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
        {/* h-[58vh] is the flex-basis; `grow` lets it FILL the viewport in the
            Explore flex chain (no dead block below), while Canvas (block
            context) keeps the 58vh height. No max cap — tall screens win. */}
        <div className="flex h-[58vh] min-h-[240px] grow flex-col overflow-y-auto">
          <div className={cn("min-h-0 grow flex-col", lens === "conversation" ? "flex" : "hidden")}>
            <ConversationView sessionId={sessionId} />
          </div>
          {visited.has("trace") && (
            <div className={lens === "trace" ? undefined : "hidden"}>
              <TurnTraceView records={records} />
            </div>
          )}
          {visited.has("subagents") && (
            <div className={lens === "subagents" ? undefined : "hidden"}>
              <SubagentsSection records={records} onOpen={onOpenSubagent} />
            </div>
          )}
          {visited.has("details") && (
            <div className={lens === "details" ? undefined : "hidden"}>
              <SessionDetailPanel sessionId={sessionId} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
