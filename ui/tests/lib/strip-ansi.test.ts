/**
 * BDD specs for ANSI escape code stripping.
 *
 * 7.9% of Bash tool results contain ANSI escape sequences (colors,
 * cursor movement, bold/reset). These render as literal garbage
 * characters in the UI. Strip them before display.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { stripAnsi } from "@/lib/strip-ansi";

describe("stripAnsi", () => {
  // ── Color codes ──────────────────────────────────────────────

  it("should strip SGR color codes (e.g., \\x1b[32m green)", () =>
    scenario(
      () => "\x1b[32mPASSED\x1b[0m",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("PASSED"),
    ));

  it("should strip multiple color codes in sequence", () =>
    scenario(
      () => "\x1b[1m\x1b[31merror\x1b[0m: something failed",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("error: something failed"),
    ));

  it("should strip 256-color codes (e.g., \\x1b[38;5;196m)", () =>
    scenario(
      () => "\x1b[38;5;196mred text\x1b[0m",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("red text"),
    ));

  it("should strip 24-bit RGB color codes (e.g., \\x1b[38;2;255;0;0m)", () =>
    scenario(
      () => "\x1b[38;2;255;0;0mred text\x1b[0m",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("red text"),
    ));

  // ── Text attributes ──────────────────────────────────────────

  it("should strip bold, dim, italic, underline, blink, reverse", () =>
    scenario(
      () => "\x1b[1mbold\x1b[2mdim\x1b[3mitalic\x1b[4munderline\x1b[7mreverse\x1b[0m",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("bolddimitalicunderlinereverse"),
    ));

  // ── Cursor movement ──────────────────────────────────────────

  it("should strip cursor movement sequences", () =>
    scenario(
      () => "\x1b[2Jhello\x1b[H\x1b[3Aworld",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("helloworld"),
    ));

  // ── Real-world cargo test output ──────────────────────────────

  it("should clean cargo test output", () =>
    scenario(
      () =>
        "test result: \x1b[32mok\x1b[0m. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s",
      (input) => stripAnsi(input),
      (result) =>
        expect(result).toBe(
          "test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s",
        ),
    ));

  it("should clean npm test output with multiple colors", () =>
    scenario(
      () => " \x1b[32m✓\x1b[39m tests/foo.test.ts \x1b[2m(\x1b[22m\x1b[2m5 tests\x1b[22m\x1b[2m)\x1b[22m\x1b[32m 7\x1b[2mms\x1b[22m\x1b[39m",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe(" ✓ tests/foo.test.ts (5 tests) 7ms"),
    ));

  // ── Edge cases ────────────────────────────────────────────────

  it("should return empty string for empty input", () =>
    scenario(
      () => "",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe(""),
    ));

  it("should return plain text unchanged", () =>
    scenario(
      () => "just normal text\nwith newlines",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("just normal text\nwith newlines"),
    ));

  it("should handle null/undefined gracefully", () => {
    expect(stripAnsi(null as unknown as string)).toBe("");
    expect(stripAnsi(undefined as unknown as string)).toBe("");
  });

  // ── OSC sequences (terminal title, hyperlinks) ────────────────

  it("should strip OSC sequences (e.g., terminal title)", () =>
    scenario(
      () => "\x1b]0;Terminal Title\x07rest of output",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("rest of output"),
    ));

  it("should strip OSC hyperlinks", () =>
    scenario(
      () => "\x1b]8;;https://example.com\x07link text\x1b]8;;\x07",
      (input) => stripAnsi(input),
      (result) => expect(result).toBe("link text"),
    ));

  // ── Performance ──────────────────────────────────────────────

  it("should strip a 100KB string with ANSI codes in < 5ms", () => {
    // Build a large string with ANSI codes every ~50 chars
    const chunk = "\x1b[32mgreen text here\x1b[0m normal text follows. ";
    const input = chunk.repeat(2500); // ~100KB
    expect(input.length).toBeGreaterThan(100_000);

    const start = performance.now();
    const result = stripAnsi(input);
    const ms = performance.now() - start;

    expect(result).not.toContain("\x1b");
    expect(ms).toBeLessThan(5);
  });
});
