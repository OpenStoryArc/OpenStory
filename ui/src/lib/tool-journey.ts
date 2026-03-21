/** Pure transforms for Tool Journey visualization. */

export interface ToolStep {
  readonly tool: string;
  readonly file: string | null;
  readonly timestamp: string;
}

/** A group of consecutive same-tool steps. */
export interface ToolGroup {
  readonly tool: string;
  readonly count: number;
  readonly files: readonly string[];
  readonly startTime: string;
  readonly endTime: string;
}

/** A gap between tool groups indicating a pause. */
export interface TimeGap {
  readonly durationMs: number;
  readonly label: string;
}

/** Element in the journey sequence: either a tool group or a gap marker. */
export type JourneyElement =
  | { readonly kind: "group"; readonly group: ToolGroup }
  | { readonly kind: "gap"; readonly gap: TimeGap };

/** Group consecutive same-tool steps into ToolGroups. */
export function groupConsecutiveTools(steps: readonly ToolStep[]): ToolGroup[] {
  if (steps.length === 0) return [];

  const groups: ToolGroup[] = [];
  let current = steps[0]!;
  let count = 1;
  let files: string[] = current.file ? [current.file] : [];
  let startTime = current.timestamp;

  for (let i = 1; i < steps.length; i++) {
    const step = steps[i]!;
    if (step.tool === current.tool) {
      count++;
      if (step.file && !files.includes(step.file)) files.push(step.file);
    } else {
      groups.push({ tool: current.tool, count, files, startTime, endTime: steps[i - 1]!.timestamp });
      current = step;
      count = 1;
      files = step.file ? [step.file] : [];
      startTime = step.timestamp;
    }
  }
  groups.push({ tool: current.tool, count, files, startTime, endTime: steps[steps.length - 1]!.timestamp });

  return groups;
}

/** Default gap threshold: 30 seconds. */
const DEFAULT_GAP_THRESHOLD_MS = 30_000;

/** Build the journey sequence with gap markers between groups that exceed the threshold. */
export function buildJourneySequence(
  groups: readonly ToolGroup[],
  gapThresholdMs: number = DEFAULT_GAP_THRESHOLD_MS,
): JourneyElement[] {
  if (groups.length === 0) return [];

  const elements: JourneyElement[] = [{ kind: "group", group: groups[0]! }];

  for (let i = 1; i < groups.length; i++) {
    const prev = groups[i - 1]!;
    const curr = groups[i]!;
    const gapMs = new Date(curr.startTime).getTime() - new Date(prev.endTime).getTime();

    if (gapMs >= gapThresholdMs) {
      elements.push({ kind: "gap", gap: { durationMs: gapMs, label: formatGapDuration(gapMs) } });
    }
    elements.push({ kind: "group", group: curr });
  }

  return elements;
}

/** Format a gap duration as a compact label. */
export function formatGapDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  return `${hours}h`;
}

/** Truncate a file path to just the basename, or shorten if still too long. */
export function truncateFilePath(path: string, maxLen: number = 30): string {
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/");
  const basename = parts[parts.length - 1] ?? path;
  if (basename.length <= maxLen) return basename;
  return basename.slice(0, maxLen - 1) + "…";
}
