import { memo } from "react";
import { compactTime } from "@/lib/time";

interface UserMessageProps {
  text: string;
  timestamp?: string;
}

export const UserMessage = memo(function UserMessage({
  text,
  timestamp,
}: UserMessageProps) {
  return (
    <div className="flex gap-3 px-4 py-3">
      <div className="w-8 h-8 rounded-full bg-[#7aa2f7] flex items-center justify-center text-xs text-[#1a1b26] font-bold flex-shrink-0">
        U
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span className="text-xs font-medium text-[#7aa2f7]">User</span>
          {timestamp && (
            <span className="text-xs text-[#565f89]">
              {compactTime(timestamp)}
            </span>
          )}
        </div>
        <div className="text-sm text-[#c0caf5] whitespace-pre-wrap break-words">
          {text}
        </div>
      </div>
    </div>
  );
});
