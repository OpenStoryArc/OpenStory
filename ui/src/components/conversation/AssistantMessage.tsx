import { memo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { compactTime } from "@/lib/time";

interface AssistantMessageProps {
  text: string;
  model?: string;
  timestamp?: string;
  isThinking?: boolean;
}

const COLLAPSE_THRESHOLD = 300; // chars

export const AssistantMessage = memo(function AssistantMessage({
  text,
  model,
  timestamp,
  isThinking,
}: AssistantMessageProps) {
  const isLong = text.length > COLLAPSE_THRESHOLD;
  const [expanded, setExpanded] = useState(!isLong);

  return (
    <div className="flex gap-3 px-4 py-3">
      <div
        className={`w-8 h-8 rounded-full flex items-center justify-center text-xs text-[#1a1b26] font-bold flex-shrink-0 ${
          isThinking ? "bg-[#565f89]" : "bg-[#bb9af7]"
        }`}
      >
        {isThinking ? "T" : "A"}
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span
            className={`text-xs font-medium ${isThinking ? "text-[#565f89]" : "text-[#bb9af7]"}`}
          >
            {isThinking ? "Thinking" : "Assistant"}
          </span>
          {timestamp && (
            <span className="text-xs text-[#565f89]">
              {compactTime(timestamp)}
            </span>
          )}
          {model && (
            <span className="text-xs text-[#565f89]">{model}</span>
          )}
        </div>
        <div
          className={`text-sm prose prose-invert max-w-none ${
            !expanded ? "max-h-24 overflow-hidden relative" : ""
          }`}
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
          {!expanded && (
            <div className="absolute bottom-0 left-0 right-0 h-8 bg-gradient-to-t from-[#1a1b26]" />
          )}
        </div>
        {isLong && (
          <button
            onClick={() => setExpanded(!expanded)}
            className="text-xs text-[#7aa2f7] hover:underline mt-1"
          >
            {expanded ? "Collapse" : "Expand"}
          </button>
        )}
      </div>
    </div>
  );
});
