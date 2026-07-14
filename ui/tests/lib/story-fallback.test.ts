/**
 * Spec: Story feed mode — the render decision for the Story view.
 *
 * The Story view renders `turn.sentence` patterns. Some sessions have records
 * but zero patterns (federated backfill, older imports) → Story was blank.
 * `storyFeed` decides what the feed shows: the sentence cards, a structural-turn
 * fallback (derived client-side from records via `extractCycles`), a loading
 * state, or the genuine empty state.
 *
 * Pure function → BDD-tested here; the component just renders the mode.
 */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { storyFeed } from "@/lib/story-fallback";
import type { EvalApplyCycle } from "@/lib/eval-apply";

const cycles: EvalApplyCycle[] = [
  { cycleNumber: 1, evalText: "Looked at the config", tools: [{ name: "Read", summary: "config.rs" }], isTerminal: false },
  { cycleNumber: 2, evalText: "Done", tools: [], isTerminal: true },
];

const base = {
  selectedSession: "sess-1" as string | null,
  loadingSentences: false,
  sentenceCount: 0,
  loadingFallback: false,
  fallbackCycles: null as EvalApplyCycle[] | null,
};

describe("storyFeed — render mode decision", () => {
  it("is loading while sentences are still being fetched", () => {
    scenario(
      () => ({ ...base, loadingSentences: true, sentenceCount: 0 }),
      (args) => storyFeed(args),
      (feed) => expect(feed.kind).toBe("loading"),
    );
  });

  it("shows sentences when the session has any", () => {
    scenario(
      () => ({ ...base, sentenceCount: 12 }),
      (args) => storyFeed(args),
      (feed) => expect(feed.kind).toBe("sentences"),
    );
  });

  it("shows loading (not empty) while the structural fallback is being fetched", () => {
    scenario(
      () => ({ ...base, sentenceCount: 0, loadingFallback: true }),
      (args) => storyFeed(args),
      (feed) => expect(feed.kind).toBe("loading"),
    );
  });

  it("falls back to structural turns when there are no sentences but records yield cycles", () => {
    scenario(
      () => ({ ...base, sentenceCount: 0, fallbackCycles: cycles }),
      (args) => storyFeed(args),
      (feed) => {
        expect(feed.kind).toBe("structural");
        if (feed.kind === "structural") expect(feed.cycles).toHaveLength(2);
      },
    );
  });

  it("prefers real sentences over the fallback when both are present", () => {
    scenario(
      () => ({ ...base, sentenceCount: 5, fallbackCycles: cycles }),
      (args) => storyFeed(args),
      (feed) => expect(feed.kind).toBe("sentences"),
    );
  });

  it("is empty (with a selection) when the fallback fetch found nothing", () => {
    scenario(
      () => ({ ...base, sentenceCount: 0, fallbackCycles: [] }),
      (args) => storyFeed(args),
      (feed) => {
        expect(feed.kind).toBe("empty");
        if (feed.kind === "empty") expect(feed.selected).toBe(true);
      },
    );
  });

  it("is empty (no selection) when no session is chosen", () => {
    scenario(
      () => ({ ...base, selectedSession: null, sentenceCount: 0 }),
      (args) => storyFeed(args),
      (feed) => {
        expect(feed.kind).toBe("empty");
        if (feed.kind === "empty") expect(feed.selected).toBe(false);
      },
    );
  });
});
