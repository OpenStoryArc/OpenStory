import { useEffect } from "react";
import type { ViewRecord, ToolCall } from "@/types/view-record";
import { viewRecordLabel } from "@/lib/view-record-transforms";
import { isGitBashRecord } from "@/lib/view-record-transforms";
import { formatTime } from "@/lib/time";
import { GitCommandDetail } from "./GitCommandDetail";

interface EventDetailProps {
  record: ViewRecord;
  onClose: () => void;
}

export function EventDetail({ record, onClose }: EventDetailProps) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  // Delegate to GitCommandDetail for git bash records
  if (isGitBashRecord(record)) {
    return <GitCommandDetail record={record} onClose={onClose} />;
  }

  const toolName = record.record_type === "tool_call"
    ? (record.payload as ToolCall).name
    : undefined;

  return (
    <div className="h-full bg-[#24283b] p-4 overflow-y-auto">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-[#c0caf5]">
          {viewRecordLabel(record.record_type)}
          {toolName && (
            <span className="ml-2 text-[#2ac3de]">{toolName}</span>
          )}
        </h3>
        <button
          onClick={onClose}
          className="text-xs text-[#565f89] hover:text-[#c0caf5] px-2 py-1"
        >
          Close
        </button>
      </div>
      <div className="grid grid-cols-2 gap-2 text-xs mb-3">
        <div>
          <span className="text-[#565f89]">Time: </span>
          <span>{formatTime(record.timestamp)}</span>
        </div>
        <div>
          <span className="text-[#565f89]">ID: </span>
          <span className="font-mono">{record.id.slice(0, 8)}</span>
        </div>
        <div>
          <span className="text-[#565f89]">Seq: </span>
          <span>{record.seq}</span>
        </div>
      </div>
      <details open className="text-xs">
        <summary className="text-[#565f89] cursor-pointer mb-2">
          Record Data
        </summary>
        <pre className="bg-[#1a1b26] rounded p-3 overflow-x-auto text-[#c0caf5] whitespace-pre-wrap break-words">
          {JSON.stringify(record.payload, null, 2)}
        </pre>
      </details>
    </div>
  );
}
