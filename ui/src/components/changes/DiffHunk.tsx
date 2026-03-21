import type { FileHunk } from "@/lib/changes";

interface DiffHunkProps {
  hunk: FileHunk;
}

export function DiffHunk({ hunk }: DiffHunkProps) {
  const timeLabel = new Date(hunk.timestamp).toLocaleTimeString();

  return (
    <div className="mb-4 border border-[#2f3348] rounded">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-[#24283b] text-xs text-[#565f89] border-b border-[#2f3348]">
        <span>
          {hunk.tool}
          {hunk.replaceAll ? " (replace all)" : ""}
        </span>
        <span>{timeLabel}</span>
      </div>

      {/* Diff content */}
      <div className="text-xs font-mono">
        {hunk.oldText !== null ? (
          <>
            <pre className="bg-red-900/30 text-red-300 px-3 py-2 overflow-x-auto whitespace-pre-wrap break-words">
              {hunk.oldText}
            </pre>
            <pre className="bg-green-900/30 text-green-300 px-3 py-2 overflow-x-auto whitespace-pre-wrap break-words">
              {hunk.newText}
            </pre>
          </>
        ) : (
          <>
            <div className="px-3 py-1 text-[#565f89] text-[10px] bg-[#1a1b26]">
              New file content
            </div>
            <pre className="bg-green-900/30 text-green-300 px-3 py-2 overflow-x-auto whitespace-pre-wrap break-words">
              {hunk.newText}
            </pre>
          </>
        )}
      </div>
    </div>
  );
}
