import { describe, it, expect } from "vitest";
import { toolColor } from "@/lib/tool-colors";

const TOOL_COLOR_TABLE: [string, string][] = [
  ["Read", "#7aa2f7"],
  ["Edit", "#7dcfff"],
  ["Write", "#7aa2f7"],
  ["Glob", "#7dcfff"],
  ["Grep", "#7aa2f7"],
  ["Bash", "#ff9e64"],
  ["WebSearch", "#2ac3de"],
  ["WebFetch", "#2ac3de"],
  ["Agent", "#bb9af7"],
  ["NotebookEdit", "#9ece6a"],
  ["Other", "#565f89"],
  // fallback
  ["UnknownTool", "#565f89"],
  ["", "#565f89"],
];

describe("toolColor — boundary table", () => {
  it.each(TOOL_COLOR_TABLE)("toolColor(%s) → %s", (name, expected) => {
    expect(toolColor(name)).toBe(expected);
  });
});
