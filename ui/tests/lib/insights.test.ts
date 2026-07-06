import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { answer, QUESTIONS } from "@/lib/insights";
import type { StorySession } from "@/lib/story-api";

function sess(id: string, over: Partial<StorySession> = {}): StorySession {
  return { session_id: id, project_name: "P", origin_agent: "claude-code", status: "completed", event_count: 10, start_time: "2026-06-30T09:00:00.000Z", last_event: "2026-06-30T10:00:00.000Z", ...over };
}
const NOW = Date.parse("2026-06-30T12:00:00.000Z");

describe("insights.answer", () => {
  it("ranks the biggest token burners", () => {
    scenario(
      () => answer([sess("a", { total_output_tokens: 100 }), sess("b", { total_output_tokens: 9000 })], "tokens", NOW),
      (ans) => ans,
      (ans) => {
        expect(ans.items[0]!.sessionId).toBe("b");
        expect(ans.items[0]!.value).toMatch(/9/);
      },
    );
  });

  it("finds what's running now (ongoing sessions)", () => {
    scenario(
      () => answer([sess("done", { status: "completed" }), sess("live", { status: "ongoing", last_event: null })], "ongoing", NOW),
      (ans) => ans,
      (ans) => {
        expect(ans.items).toHaveLength(1);
        expect(ans.items[0]!.sessionId).toBe("live");
      },
    );
  });

  it("scopes 'today' to the current local day", () => {
    scenario(
      () => answer(
        [sess("t", { start_time: "2026-06-30T08:00:00.000Z" }), sess("old", { start_time: "2026-06-01T08:00:00.000Z" })],
        "today", NOW,
      ),
      (ans) => ans.items.map((i) => i.sessionId),
      (ids) => expect(ids).toEqual(["t"]),
    );
  });

  it("summarizes agents with per-event output efficiency", () => {
    scenario(
      () => answer(
        [sess("a", { origin_agent: "claude-code", event_count: 10, total_output_tokens: 1000 }),
         sess("b", { origin_agent: "openactor", event_count: 5, total_output_tokens: 0 })],
        "agents", NOW,
      ),
      (ans) => ans,
      (ans) => {
        const cc = ans.items.find((i) => i.label === "claude-code")!;
        expect(cc.value).toMatch(/100 out-tok\/event/);
        const oa = ans.items.find((i) => i.label === "openactor")!;
        expect(oa.value).toMatch(/no token telemetry/);
      },
    );
  });

  it("every question id yields a titled answer", () => {
    for (const q of QUESTIONS) {
      const a = answer([sess("x")], q.id, NOW);
      expect(a.title.length).toBeGreaterThan(0);
    }
  });
});
