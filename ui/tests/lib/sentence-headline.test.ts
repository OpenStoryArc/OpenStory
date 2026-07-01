import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { sentenceHeadline } from "@/lib/story";
import type { PatternView } from "@/types/wire-record";

function sent(meta: Record<string, unknown>, label = "L"): PatternView {
  return { type: "turn.sentence", label, session_id: "s", events: [], metadata: meta };
}

describe("sentenceHeadline", () => {
  it("builds 'verb object' from the sentence metadata", () => {
    scenario(
      () => sent({ subject: "Claude", verb: "edited", object: "5 source files" }),
      (p) => sentenceHeadline(p),
      (h) => expect(h.text).toBe("edited 5 source files"),
    );
  });

  it("returns the first line of the adverbial as the 'because', de-quoted + capped", () => {
    scenario(
      () => sent({ verb: "wrote", object: "a test", adverbial: '"polishing that would help\nsecond line"' }),
      (p) => sentenceHeadline(p),
      (h) => {
        expect(h.text).toBe("wrote a test");
        expect(h.because).toBe("polishing that would help");
      },
    );
  });

  it("falls back to the label when verb/object are absent", () => {
    scenario(
      () => sent({}, "Turn 3"),
      (p) => sentenceHeadline(p),
      (h) => {
        expect(h.text).toBe("Turn 3");
        expect(h.because).toBeNull();
      },
    );
  });
});
