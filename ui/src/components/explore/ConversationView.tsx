/** Conversation view — paired turns fetched from REST, rendered as collapsible cards. */

import { useState, useEffect, useMemo } from "react";
import type { PairedConversation } from "@/types/view-record";
import { groupIntoTurns, totalToolCalls } from "@/lib/conversation";
import { TurnCard } from "./TurnCard";

interface ConversationViewProps {
  sessionId: string;
}

export function ConversationView({ sessionId }: ConversationViewProps) {
  const [data, setData] = useState<PairedConversation | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setData(null);

    fetch(`/api/sessions/${sessionId}/conversation`)
      .then((r) => r.json())
      .then((d: PairedConversation) => {
        if (!cancelled) {
          setData(d);
          setLoading(false);
        }
      })
      .catch(() => {
        if (!cancelled) setLoading(false);
      });

    return () => { cancelled = true; };
  }, [sessionId]);

  const turns = useMemo(
    () => (data ? groupIntoTurns(data.entries) : []),
    [data],
  );

  if (loading) {
    return <div className="p-4 text-xs text-[#565f89]">Loading conversation...</div>;
  }

  if (turns.length === 0) {
    return <div className="p-4 text-xs text-[#565f89]">No conversation data</div>;
  }

  const toolCount = totalToolCalls(turns);

  return (
    <div className="p-3 space-y-3" data-testid="conversation-view">
      <div className="text-[10px] text-[#565f89] px-1">
        {turns.length} turn{turns.length !== 1 ? "s" : ""}
        {toolCount > 0 && ` · ${toolCount} tool call${toolCount !== 1 ? "s" : ""}`}
      </div>
      {turns.map((turn, i) => (
        <TurnCard key={i} turn={turn} index={i} />
      ))}
    </div>
  );
}
