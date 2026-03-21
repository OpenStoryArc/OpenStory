import type { FileChange } from "@/lib/changes";

interface FileChangeListProps {
  files: FileChange[];
  selectedPath: string | null;
  onSelect: (path: string) => void;
}

export function FileChangeList({
  files,
  selectedPath,
  onSelect,
}: FileChangeListProps) {
  return (
    <div className="overflow-y-auto h-full">
      {files.map((file) => {
        const isSelected = file.filePath === selectedPath;
        const time = new Date(file.lastChanged).toLocaleTimeString();

        return (
          <button
            key={file.filePath}
            onClick={() => onSelect(file.filePath)}
            className={`w-full text-left px-3 py-2 border-b border-[#2f3348] transition-colors ${
              isSelected
                ? "bg-[#24283b] text-[#c0caf5]"
                : "text-[#a9b1d6] hover:bg-[#1f2335]"
            }`}
          >
            <div className="flex items-center justify-between">
              <span className="text-sm truncate font-mono">
                {file.fileName}
              </span>
              <span className="text-xs text-[#565f89] ml-2 shrink-0">
                {file.hunks.length} {file.hunks.length === 1 ? "change" : "changes"}
              </span>
            </div>
            <div className="text-xs text-[#565f89] truncate mt-0.5">
              {file.filePath}
            </div>
            <div className="text-[10px] text-[#565f89] mt-0.5">{time}</div>
          </button>
        );
      })}
    </div>
  );
}
