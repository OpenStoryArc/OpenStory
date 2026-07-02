import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { isRichMessage } from "@/lib/present-message";

describe("isRichMessage — should the present banner offer expand/rich rendering", () => {
  it("is false for a short single-line message", () => {
    scenario(
      () => "Jump to the latest session →",
      (m) => isRichMessage(m),
      (rich) => expect(rich).toBe(false),
    );
  });

  it("is false for empty / whitespace", () => {
    scenario(
      () => "   ",
      (m) => isRichMessage(m),
      (rich) => expect(rich).toBe(false),
    );
  });

  it("is true when the message spans multiple lines", () => {
    scenario(
      () => "Line one\nLine two",
      (m) => isRichMessage(m),
      (rich) => expect(rich).toBe(true),
    );
  });

  it("is true when the message contains a fenced code block", () => {
    scenario(
      () => "Run this:\n```bash\ncurl localhost:3002\n```",
      (m) => isRichMessage(m),
      (rich) => expect(rich).toBe(true),
    );
  });

  it("is true for a long single line (past the one-line budget)", () => {
    scenario(
      () => "x".repeat(200),
      (m) => isRichMessage(m),
      (rich) => expect(rich).toBe(true),
    );
  });
});
