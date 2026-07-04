import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type {
  PairedConversation,
  ConversationEntry,
} from "@/types/view-record";
import { UserMessage } from "./UserMessage";
import { AssistantMessage } from "./AssistantMessage";
import { ToolCallBlock } from "./ToolCallBlock";
import { FilterPills } from "@/components/ui/FilterPills";
import { Skeleton } from "@/components/ui/skeleton";
import { CONVERSATION_FACETS, conversationEntryMatches, facetCounts, type ConversationFacet } from "@/lib/conversation-facets";

interface ConversationViewProps {
  sessionId: string;
}

/** Events per page — the window /conversation is asked for. One page is
 *  plenty for reading the recent story; older history loads on demand. */
const CONVERSATION_PAGE_EVENTS = 500;

interface PagedConversation extends PairedConversation {
  next_before_seq?: number;
}

export function ConversationView({ sessionId }: ConversationViewProps) {
  const [entries, setEntries] = useState<ConversationEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [nextBeforeSeq, setNextBeforeSeq] = useState<number | null>(null);
  const [facet, setFacet] = useState<ConversationFacet>("all");
  const parentRef = useRef<HTMLDivElement>(null);

  // Live-tab facet filtering over the paired entries (same experience as Live).
  const counts = useMemo(() => facetCounts(entries), [entries]);
  const visible = useMemo(
    () => (facet === "all" ? entries : entries.filter((e) => conversationEntryMatches(e, facet))),
    [entries, facet],
  );

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setEntries([]);
    setNextBeforeSeq(null);
    fetch(`/api/sessions/${sessionId}/conversation?limit=${CONVERSATION_PAGE_EVENTS}`)
      .then((r) => r.json())
      .then((data: PagedConversation) => {
        if (!cancelled) {
          setEntries(data.entries ?? []);
          setNextBeforeSeq(data.next_before_seq ?? null);
          setLoading(false);
        }
      })
      .catch(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  const loadOlder = useCallback(() => {
    if (nextBeforeSeq === null || loadingOlder) return;
    setLoadingOlder(true);
    fetch(
      `/api/sessions/${sessionId}/conversation?limit=${CONVERSATION_PAGE_EVENTS}&before_seq=${nextBeforeSeq}`,
    )
      .then((r) => r.json())
      .then((data: PagedConversation) => {
        setEntries((cur) => [...(data.entries ?? []), ...cur]);
        setNextBeforeSeq(data.next_before_seq ?? null);
        setLoadingOlder(false);
      })
      .catch(() => setLoadingOlder(false));
  }, [sessionId, nextBeforeSeq, loadingOlder]);

  const virtualizer = useVirtualizer({
    count: visible.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 100,
    overscan: 10,
  });

  const renderEntry = useCallback(
    (entry: ConversationEntry) => {
      switch (entry.entry_type) {
        case "user_message":
          return (
            <UserMessage
              text={
                typeof entry.payload.content === "string"
                  ? entry.payload.content
                  : entry.payload.content
                      ?.filter((b) => b.type === "text")
                      .map((b) => b.text ?? "")
                      .join("") ?? ""
              }
              timestamp={entry.timestamp}
            />
          );
        case "assistant_message":
          return (
            <AssistantMessage
              text={
                entry.payload.content
                  ?.filter((b) => b.type === "text")
                  .map((b) => b.text ?? "")
                  .join("") ?? ""
              }
              model={entry.payload.model}
              timestamp={entry.timestamp}
            />
          );
        case "reasoning":
          return (
            <AssistantMessage
              text={entry.payload.content ?? entry.payload.summary.join("\n") ?? ""}
              isThinking
              timestamp={entry.timestamp}
            />
          );
        case "tool_roundtrip":
          return (
            <ToolCallBlock
              call={entry.call.payload as import("@/types/view-record").ToolCall}
              result={
                entry.result?.record_type === "tool_result"
                  ? (entry.result.payload as import("@/types/view-record").ToolResult)
                  : undefined
              }
            />
          );
        case "token_usage":
        case "system":
          return null;
      }
    },
    [],
  );

  if (loading) {
    // Shaped skeleton: the silhouette of a conversation (alternating
    // bubbles + a tool row), not a bare spinner line.
    return (
      <div className="flex h-full flex-col gap-3 p-3" data-testid="conversation-loading">
        <Skeleton className="h-10 w-3/5 self-end rounded-lg" />
        <Skeleton className="h-16 w-4/5 rounded-lg" />
        <Skeleton className="h-8 w-2/3 rounded" />
        <Skeleton className="h-10 w-1/2 self-end rounded-lg" />
        <Skeleton className="h-20 w-4/5 rounded-lg" />
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-[#565f89] text-sm">
        No conversation data
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full min-h-0">
      <FilterPills facets={CONVERSATION_FACETS} active={facet} counts={counts} onSelect={setFacet} />
      {nextBeforeSeq !== null && (
        <button
          type="button"
          data-testid="load-older"
          onClick={loadOlder}
          disabled={loadingOlder}
          className="w-full border-b border-[#2f3348] px-3 py-1.5 text-[11px] text-[#7aa2f7] hover:bg-[#2f3348] disabled:opacity-50"
        >
          {loadingOlder ? "Loading older…" : "↑ Load older history"}
        </button>
      )}
      <div ref={parentRef} className="flex-1 overflow-y-auto">
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: "100%",
          position: "relative",
        }}
      >
        {virtualizer.getVirtualItems().map((vItem) => {
          const item = visible[vItem.index]!;
          return (
            <div
              key={vItem.key}
              data-index={vItem.index}
              ref={virtualizer.measureElement}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${vItem.start}px)`,
              }}
            >
              {renderEntry(item)}
            </div>
          );
        })}
      </div>
      </div>
    </div>
  );
}
