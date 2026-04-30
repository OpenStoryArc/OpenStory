import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { isCatNumbered, stripLineNumbers, extractStartLineNumber } from "@/lib/strip-line-numbers";

// ── isCatNumbered — boundary table ──────────────────────

const DETECT_TABLE: [string, string, boolean][] = [
  ["cat -n output", "     1→const x = 1;\n     2→const y = 2;\n     3→return x + y;", true],
  ["single line cat -n", "     1→hello", true],
  ["plain text", "const x = 1;\nconst y = 2;", false],
  ["empty string", "", false],
  ["mixed (mostly numbered)", "     1→a\n     2→b\n     3→c\nplain", true],
  ["mixed (mostly plain)", "plain\nplain\n     1→a", false],
  ["large line numbers", "   100→line 100\n   101→line 101", true],
  ["no arrow", "  1  hello\n  2  world", false],
  ["tab after arrow", "     1→\tcontent", true],
  ["tab-separated (no arrow)", "1\t[package]\n2\tname = \"foo\"\n3\tversion = \"0.1\"", true],
  ["tab-separated single line", "1\thello", true],
  ["tab-separated large numbers", "100\tline hundred\n101\tline 101", true],
];

describe("isCatNumbered — boundary table", () => {
  it.each(DETECT_TABLE)(
    "%s",
    (_desc, input, expected) => {
      scenario(
        () => input,
        (text) => isCatNumbered(text),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── stripLineNumbers — boundary table ──────────────────────

const STRIP_TABLE: [string, string, string][] = [
  [
    "standard cat -n output",
    "     1→const x = 1;\n     2→const y = 2;\n     3→return x + y;",
    "const x = 1;\nconst y = 2;\nreturn x + y;",
  ],
  [
    "with tab after arrow",
    "     1→\tindented content\n     2→\tmore content",
    "indented content\nmore content",
  ],
  [
    "large line numbers",
    "   100→line 100\n   101→line 101\n   102→line 102",
    "line 100\nline 101\nline 102",
  ],
  [
    "plain text (no line numbers) passes through",
    "hello world\nno numbers here",
    "hello world\nno numbers here",
  ],
  [
    "empty string",
    "",
    "",
  ],
  [
    "single line",
    "     1→only line",
    "only line",
  ],
  [
    "preserves empty lines",
    "     1→first\n     2→\n     3→third",
    "first\n\nthird",
  ],
  [
    "tab-separated (Agent tool format)",
    "1\t[package]\n2\tname = \"open-story-server\"\n3\tversion = \"0.1.0\"",
    "[package]\nname = \"open-story-server\"\nversion = \"0.1.0\"",
  ],
  [
    "tab-separated large numbers",
    "100\tline hundred\n101\tline 101",
    "line hundred\nline 101",
  ],
];

// ── extractStartLineNumber — boundary table ──────────────────────

const START_LINE_TABLE: [string, string, number][] = [
  ["starts at 1", "     1→first line\n     2→second line", 1],
  ["starts at offset", "    50→line fifty\n    51→line fifty-one", 50],
  ["starts at 100", "   100→line hundred", 100],
  ["no line numbers", "plain text here", 1],
  ["empty string", "", 1],
  ["single digit", "     5→fifth line", 5],
  ["tab-separated starts at 1", "1\t[package]\n2\tname", 1],
  ["tab-separated offset", "42\tfirst\n43\tsecond", 42],
];

describe("extractStartLineNumber — boundary table", () => {
  it.each(START_LINE_TABLE)(
    "%s",
    (_desc, input, expected) => {
      scenario(
        () => input,
        (text) => extractStartLineNumber(text),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});

// ── stripLineNumbers — boundary table ──────────────────────

describe("stripLineNumbers — boundary table", () => {
  it.each(STRIP_TABLE)(
    "%s",
    (_desc, input, expected) => {
      scenario(
        () => input,
        (text) => stripLineNumbers(text),
        (result) => expect(result).toBe(expected),
      );
    },
  );
});
