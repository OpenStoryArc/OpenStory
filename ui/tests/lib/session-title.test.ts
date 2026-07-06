import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { sessionTitle } from "@/lib/session-title";
import type { StorySession } from "@/lib/story-api";

function sess(over: Partial<StorySession> = {}): StorySession {
  return { session_id: "2a0d4337-ce4b-4797-885b-e990b1523426", project_name: "P", origin_agent: "claude-code", status: "completed", ...over };
}

describe("sessionTitle", () => {
  it("humanizes a harness command wrapper into a clean slash command", () => {
    scenario(
      () => sess({ first_prompt: "<command-message>loop</command-message><command-name>loop</command-name>" }),
      (s) => sessionTitle(s),
      (t) => {
        expect(t).toContain("/loop");
        expect(t).not.toContain("<command");
      },
    );
  });

  it("prefers label over first_prompt", () => {
    scenario(
      () => sess({ label: "Fix the auth bug", first_prompt: "raw prompt text" }),
      (s) => sessionTitle(s),
      (t) => expect(t).toBe("Fix the auth bug"),
    );
  });

  it("falls back to a short session id when there is no label or prompt", () => {
    scenario(
      () => sess({ label: undefined, first_prompt: undefined }),
      (s) => sessionTitle(s),
      (t) => expect(t).toBe("2a0d4337"),
    );
  });

  it("trims whitespace-only labels to the id fallback", () => {
    scenario(
      () => sess({ label: "   ", first_prompt: "" }),
      (s) => sessionTitle(s),
      (t) => expect(t).toBe("2a0d4337"),
    );
  });
});
