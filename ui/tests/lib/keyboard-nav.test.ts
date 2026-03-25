import { describe, it, expect } from "vitest";
import { nextCardIndex } from "@/lib/keyboard-nav";

// ---------------------------------------------------------------------------
// Helper: row factory
// ---------------------------------------------------------------------------

type Row = { category: string };

function rows(...categories: string[]): Row[] {
  return categories.map((category) => ({ category }));
}

// ---------------------------------------------------------------------------
// nextCardIndex — boundary table
// ---------------------------------------------------------------------------

describe("nextCardIndex", () => {
  const CASES: [string, Row[], number | null, "up" | "down", number | null][] = [
    // [description, rows, currentIndex, direction, expected]
    ["empty rows, down",              [],                                    null, "down", null],
    ["empty rows, up",                [],                                    null, "up",   null],
    ["all turns, down",               rows("turn", "turn", "turn"),          null, "down", null],
    ["all turns, up",                 rows("turn", "turn", "turn"),          null, "up",   null],
    ["single event, null down",       rows("prompt"),                        null, "down", 0],
    ["single event, null up",         rows("prompt"),                        null, "up",   0],
    ["single event, down stays",      rows("prompt"),                        0,    "down", 0],
    ["single event, up stays",        rows("prompt"),                        0,    "up",   0],
    ["two events, down",              rows("prompt", "response"),            0,    "down", 1],
    ["two events, up",                rows("prompt", "response"),            1,    "up",   0],
    ["skip turn going down",          rows("prompt", "turn", "response"),    0,    "down", 2],
    ["skip turn going up",            rows("prompt", "turn", "response"),    2,    "up",   0],
    ["skip multiple turns down",      rows("prompt", "turn", "turn", "tool"),0,    "down", 3],
    ["skip multiple turns up",        rows("prompt", "turn", "turn", "tool"),3,    "up",   0],
    ["down from last event stays",    rows("prompt", "response"),            1,    "down", 1],
    ["up from first event stays",     rows("prompt", "response"),            0,    "up",   0],
    ["null down, first is turn",      rows("turn", "prompt", "response"),    null, "down", 1],
    ["null up, last is turn",         rows("prompt", "response", "turn"),    null, "up",   1],
    ["down past trailing turns",      rows("prompt", "turn", "turn"),        0,    "down", 0],
    ["up past leading turns",         rows("turn", "turn", "response"),      2,    "up",   2],
    ["mixed sequence down",           rows("prompt", "turn", "tool", "turn", "response"), 0, "down", 2],
    ["mixed sequence continued",      rows("prompt", "turn", "tool", "turn", "response"), 2, "down", 4],
    ["mixed sequence up",             rows("prompt", "turn", "tool", "turn", "response"), 4, "up", 2],
  ];

  it.each(CASES)("%s", (_desc, rowsArr, current, direction, expected) => {
    expect(nextCardIndex(rowsArr, current, direction)).toBe(expected);
  });
});
