import type { ViewRecord, ToolCall, ToolResult, EditInput, WriteInput, ReadInput } from "@/types/view-record";

export interface FileHunk {
  timestamp: string;
  eventId: string;
  tool: "Edit" | "Write";
  oldText: string | null;
  newText: string;
  replaceAll?: boolean;
}

export interface FileChange {
  filePath: string;
  fileName: string;
  hunks: FileHunk[];
  lastChanged: string;
}

/** Scan backward from a Write record to find the nearest Read result for the same path. */
export function pairReadWithWrite(
  records: readonly ViewRecord[],
  writeIndex: number,
): string | null {
  const writeRecord = records[writeIndex]!;
  if (!writeRecord || writeRecord.record_type !== "tool_call") return null;
  const tc = writeRecord.payload as ToolCall;
  if (tc.typed_input?.tool !== "write") return null;
  const writePath = (tc.typed_input as WriteInput).file_path;

  for (let i = writeIndex - 1; i >= 0; i--) {
    const r = records[i]!;
    if (r.record_type !== "tool_result") continue;
    // Check if preceding tool_call was a Read on the same path
    if (i > 0) {
      const prev = records[i - 1]!;
      if (prev.record_type === "tool_call") {
        const prevTc = prev.payload as ToolCall;
        if (
          prevTc.typed_input?.tool === "read" &&
          (prevTc.typed_input as ReadInput).file_path === writePath
        ) {
          const result = r.payload as ToolResult;
          return result.output ?? null;
        }
      }
    }
  }
  return null;
}

/** Extract file changes from ViewRecords. */
export function extractFileChanges(records: readonly ViewRecord[]): FileChange[] {
  const fileMap = new Map<string, FileHunk[]>();

  for (let i = 0; i < records.length; i++) {
    const r = records[i]!;
    if (r.record_type !== "tool_call") continue;

    const tc = r.payload as ToolCall;
    const typed = tc.typed_input;
    if (!typed) continue;

    if (typed.tool === "edit") {
      const input = typed as EditInput;
      const hunk: FileHunk = {
        timestamp: r.timestamp,
        eventId: r.id,
        tool: "Edit",
        oldText: input.old_string,
        newText: input.new_string,
        replaceAll: input.replace_all === true ? true : undefined,
      };
      const existing = fileMap.get(input.file_path) ?? [];
      existing.push(hunk);
      fileMap.set(input.file_path, existing);
    } else if (typed.tool === "write") {
      const input = typed as WriteInput;
      const oldText = pairReadWithWrite(records, i);
      const hunk: FileHunk = {
        timestamp: r.timestamp,
        eventId: r.id,
        tool: "Write",
        oldText,
        newText: input.content,
      };
      const existing = fileMap.get(input.file_path) ?? [];
      existing.push(hunk);
      fileMap.set(input.file_path, existing);
    }
  }

  const changes: FileChange[] = [];
  for (const [filePath, hunks] of fileMap) {
    const fileName = filePath.split("/").pop() ?? filePath;
    const lastChanged = hunks.reduce(
      (latest, h) => (h.timestamp > latest ? h.timestamp : latest),
      "",
    );
    changes.push({ filePath, fileName, hunks, lastChanged });
  }

  changes.sort((a, b) => (a.lastChanged > b.lastChanged ? -1 : 1));
  return changes;
}

/** Summary statistics for file changes. */
export function hunkStats(changes: FileChange[]): {
  filesChanged: number;
  totalEdits: number;
} {
  return {
    filesChanged: changes.length,
    totalEdits: changes.reduce((sum, c) => sum + c.hunks.length, 0),
  };
}
