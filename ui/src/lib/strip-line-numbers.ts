/** Strip `cat -n` style line numbers from Read tool output.
 *
 *  Two prefix formats are observed in the wild:
 *    Claude Code Read tool:        "     1→content here"   (arrow separator)
 *    Agent sub-agent / pi-mono:    "1\tcontent here"       (tab separator)
 *
 *  This strips either prefix to recover the original file content
 *  for syntax highlighting.
 *
 *  Pure function: string in, string out. */

/** Pattern: optional whitespace + digits + (arrow with optional tab | tab). */
const LINE_NUM_RE = /^ *\d+(?:→\t?|\t)/;

/** Check if text appears to be cat -n formatted (first few lines match the pattern). */
export function isCatNumbered(text: string): boolean {
  const lines = text.split("\n").slice(0, 5);
  if (lines.length === 0) return false;
  const matchCount = lines.filter((l) => LINE_NUM_RE.test(l)).length;
  // At least 60% of sampled lines should match
  return matchCount >= Math.ceil(lines.length * 0.6);
}

/** Strip cat -n line number prefixes from every line. */
export function stripLineNumbers(text: string): string {
  if (!text) return "";
  return text
    .split("\n")
    .map((line) => line.replace(LINE_NUM_RE, ""))
    .join("\n");
}

/** Extract the starting line number from cat -n formatted text.
 *  Returns 1 if not detectable. */
export function extractStartLineNumber(text: string): number {
  if (!text) return 1;
  const match = text.match(/^ *(\d+)(?:→|\t)/);
  if (!match) return 1;
  return parseInt(match[1]!, 10) || 1;
}
