import { memo, useState } from "react";
import { compactTime, fullTimestamp } from "@/lib/time";
import { Markdown } from "@/components/ui/Markdown";
import { MessageImages } from "./MessageImages";
import type { ResolvedImage } from "@/lib/message-images";

interface AssistantMessageProps {
  text: string;
  images?: readonly ResolvedImage[];
  model?: string;
  timestamp?: string;
  isThinking?: boolean;
}

const COLLAPSE_THRESHOLD = 300; // chars

export const AssistantMessage = memo(function AssistantMessage({
  text,
  images,
  model,
  timestamp,
  isThinking,
}: AssistantMessageProps) {
  const isLong = text.length > COLLAPSE_THRESHOLD;
  const [expanded, setExpanded] = useState(!isLong);

  return (
    <div className="flex gap-3 px-4 py-3">
      <div
        className={`w-8 h-8 rounded-full flex items-center justify-center text-xs text-[color:var(--bg)] font-bold flex-shrink-0 ${
          isThinking ? "bg-[color:var(--text-muted)]" : "bg-[color:var(--purple)]"
        }`}
      >
        {isThinking ? "T" : "A"}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span
            className={`text-xs font-medium ${isThinking ? "text-[color:var(--text-muted)]" : "text-[color:var(--purple)]"}`}
          >
            {isThinking ? "Thinking" : "Assistant"}
          </span>
          {timestamp && (
            <span className="text-xs text-[color:var(--text-muted)]">
              <span title={fullTimestamp(timestamp)}>{compactTime(timestamp)}</span>
            </span>
          )}
          {model && (
            <span className="text-xs text-[color:var(--text-muted)]">{model}</span>
          )}
        </div>
        <div
          className={`text-sm prose prose-invert max-w-none ${
            !expanded ? "max-h-24 overflow-hidden relative" : ""
          }`}
        >
          <Markdown>{text}</Markdown>
          {!expanded && (
            <div className="absolute bottom-0 left-0 right-0 h-8 bg-gradient-to-t from-[color:var(--bg)]" />
          )}
        </div>
        {isLong && (
          <button
            onClick={() => setExpanded(!expanded)}
            className="text-xs text-[color:var(--accent)] hover:underline mt-1"
          >
            {expanded ? "Collapse" : "Expand"}
          </button>
        )}
        <MessageImages images={images ?? []} />
      </div>
    </div>
  );
});
