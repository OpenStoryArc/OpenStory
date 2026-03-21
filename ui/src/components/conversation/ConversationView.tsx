import { useState, useEffect, useRef, useCallback } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type {
  PairedConversation,
  ConversationEntry,
} from "@/types/view-record";
import { UserMessage } from "./UserMessage";
import { AssistantMessage } from "./AssistantMessage";
import { ToolCallBlock } from "./ToolCallBlock";

interface ConversationViewProps {
  sessionId: string;
}

export function ConversationView({ sessionId }: ConversationViewProps) {
  const [entries, setEntries] = useState<ConversationEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    fetch(`/api/sessions/${sessionId}/conversation`)
      .then((r) => r.json())
      .then((data: PairedConversation) => {
        if (!cancelled) {
          setEntries(data.entries ?? []);
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

  const virtualizer = useVirtualizer({
    count: entries.length,
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
    return (
      <div className="flex items-center justify-center h-full text-[#565f89] text-sm">
        Loading transcript...
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
    <div ref={parentRef} className="flex-1 overflow-y-auto">
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: "100%",
          position: "relative",
        }}
      >
        {virtualizer.getVirtualItems().map((vItem) => {
          const item = entries[vItem.index]!;
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
  );
}
