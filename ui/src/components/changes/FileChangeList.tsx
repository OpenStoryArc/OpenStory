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
            className={`w-full text-left px-3 py-2 border-b border-[color:var(--divider)] transition-colors ${
              isSelected
                ? "bg-[color:var(--bg-surface)] text-[color:var(--text)]"
                : "text-[color:var(--text-bright)] hover:bg-[color:var(--bg-surface)]"
            }`}
          >
            <div className="flex items-center justify-between">
              <span className="text-sm truncate font-mono">
                {file.fileName}
              </span>
              <span className="text-xs text-[color:var(--text-muted)] ml-2 shrink-0">
                {file.hunks.length} {file.hunks.length === 1 ? "change" : "changes"}
              </span>
            </div>
            <div className="text-xs text-[color:var(--text-muted)] truncate mt-0.5">
              {file.filePath}
            </div>
            <div className="text-[10px] text-[color:var(--text-muted)] mt-0.5">{time}</div>
          </button>
        );
      })}
    </div>
  );
}
