import { memo } from "react";
import { compactTime, fullTimestamp } from "@/lib/time";
import { userMessageView } from "@/lib/harness-message";
import { Markdown } from "@/components/ui/Markdown";
import { MessageImages } from "./MessageImages";
import type { ResolvedImage } from "@/lib/message-images";

interface UserMessageProps {
  text: string;
  images?: readonly ResolvedImage[];
  timestamp?: string;
}

export const UserMessage = memo(function UserMessage({
  text,
  images,
  timestamp,
}: UserMessageProps) {
  return (
    <div className="flex gap-3 px-4 py-3">
      <div className="w-8 h-8 rounded-full bg-[color:var(--accent)] flex items-center justify-center text-xs text-[color:var(--bg)] font-bold flex-shrink-0">
        U
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-xs font-medium text-[color:var(--accent)]">User</span>
          {timestamp && (
            <span className="text-xs text-[color:var(--text-muted)]">
              <span title={fullTimestamp(timestamp)}>{compactTime(timestamp)}</span>
            </span>
          )}
        </div>
        {(() => {
          const v = userMessageView(text);
          // A slash-command reads as a clean chip; a real message renders as
          // markdown with syntax-highlighted code (the "Live experience").
          if (v.command) {
            return (
              <code className="inline-block rounded bg-[color:var(--bg)] px-2 py-0.5 font-mono text-[12px] text-[color:var(--cyan-bright)]">
                {v.command}
              </code>
            );
          }
          return (
            <Markdown className="text-sm text-[color:var(--text)] break-words [&_p]:my-1 [&_h1]:text-base [&_h1]:font-semibold [&_h2]:text-sm [&_h2]:font-semibold [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:list-decimal [&_ol]:pl-5">
              {v.body}
            </Markdown>
          );
        })()}
        <MessageImages images={images ?? []} />
      </div>
    </div>
  );
});
