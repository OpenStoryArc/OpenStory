/** Table showing file impact: path, reads, writes, sorted by total ops. */

import { sortFileImpact, fileBasename, type FileImpact } from "@/lib/session-detail";

interface FileImpactTableProps {
  files: readonly FileImpact[];
}

export function FileImpactTable({ files }: FileImpactTableProps) {
  const sorted = sortFileImpact(files);

  if (sorted.length === 0) {
    return (
      <div className="text-xs text-[#565f89] py-2" data-testid="file-impact-empty">
        No files touched
      </div>
    );
  }

  return (
    <div data-testid="file-impact-table">
      <div className="text-[10px] text-[#565f89] mb-1.5">
        Files ({sorted.length})
      </div>
      <div className="space-y-0.5 max-h-[200px] overflow-y-auto">
        {sorted.map((f) => (
          <div
            key={f.file}
            className="flex items-center gap-2 text-xs py-0.5 px-1 rounded hover:bg-[#1a1b26]"
          >
            <span className="flex-1 min-w-0 truncate font-mono text-[#a9b1d6]" title={f.file}>
              {fileBasename(f.file)}
            </span>
            {f.reads > 0 && (
              <span className="text-[#7aa2f7] text-[10px] shrink-0">
                {f.reads}R
              </span>
            )}
            {f.writes > 0 && (
              <span className="text-[#e0af68] text-[10px] shrink-0">
                {f.writes}W
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
