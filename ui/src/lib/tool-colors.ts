/** Tool name → Tokyonight color mapping for charts.
 *  File tools are blue, Bash is orange, Web* is cyan,
 *  Agent is purple, and everything else is grey. */

const TOOL_COLOR_MAP: Record<string, string> = {
  // File tools — blue family
  Read: "#7aa2f7",
  Edit: "#7dcfff",
  Write: "#7aa2f7",
  Glob: "#7dcfff",
  Grep: "#7aa2f7",

  // Execution — orange
  Bash: "#ff9e64",

  // Web — cyan
  WebSearch: "#2ac3de",
  WebFetch: "#2ac3de",

  // Delegation — purple
  Agent: "#bb9af7",

  // Notebook
  NotebookEdit: "#9ece6a",

  // Other bucket
  Other: "#565f89",
};

/** Get the chart color for a tool name. Falls back to muted grey. */
export function toolColor(name: string): string {
  return TOOL_COLOR_MAP[name] ?? "#565f89";
}
