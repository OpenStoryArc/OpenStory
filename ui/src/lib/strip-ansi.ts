/**
 * Strip ANSI escape sequences from terminal output.
 *
 * Handles SGR (colors/attributes), cursor movement, OSC (terminal title,
 * hyperlinks), and other common terminal escape sequences.
 *
 * Pure function: string in, string out.
 */

// Combined regex for all common ANSI escape sequences:
// 1. CSI sequences: ESC [ ... (letter)    — colors, cursor, erase
// 2. OSC sequences: ESC ] ... (BEL or ST) — title, hyperlinks
// 3. Simple escapes: ESC (letter)         — e.g., ESC c (reset)
const ANSI_RE =
  // eslint-disable-next-line no-control-regex
  /\x1b(?:\[[0-9;]*[a-zA-Z]|\][^\x07]*\x07|\][^\x1b]*\x1b\\|[a-zA-Z])/g;

/** Strip all ANSI escape sequences from a string. */
export function stripAnsi(input: string): string {
  if (!input) return "";
  return input.replace(ANSI_RE, "");
}
