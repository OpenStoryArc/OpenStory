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

/** Check if text appears to be cat -n / agent line-numbered file content.
 *
 *  Dense Claude Code Read: most lines match `N→` / tab form.
 *  Sparse Grok Build read_file: often only the first line (and every ~Nth)
 *  carries `N→`, with intervening lines bare — still strip-and-highlight.
 */
export function isCatNumbered(text: string): boolean {
  if (!text) return false;
  const lines = text.split("\n");
  if (lines.length === 0) return false;

  // Strong signal: first non-empty line is numbered (Grok + Claude openers).
  const first = lines.find((l) => l.length > 0);
  if (first && LINE_NUM_RE.test(first)) return true;

  // Dense cat -n: sample first 5 lines, require ≥60% match.
  const sample = lines.slice(0, 5);
  const matchCount = sample.filter((l) => LINE_NUM_RE.test(l)).length;
  if (matchCount >= Math.ceil(sample.length * 0.6)) return true;

  // Sparse numbering deeper in the file (Grok every ~10 lines).
  const window = lines.slice(0, Math.min(lines.length, 40));
  const deep = window.filter((l) => LINE_NUM_RE.test(l)).length;
  return deep >= 2;
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
