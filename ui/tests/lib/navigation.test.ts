import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import type { ViewMode, CrossLink } from "@/lib/navigation";

describe("navigation types", () => {
  it("ViewMode is 'live' or 'explore'", () => {
    scenario(
      () => ["live", "explore"] as ViewMode[],
      (modes) => modes,
      (modes) => {
        expect(modes).toContain("live");
        expect(modes).toContain("explore");
        expect(modes).toHaveLength(2);
      },
    );
  });

  it("CrossLink carries sessionId and optional eventId", () => {
    scenario(
      () => ({ sessionId: "abc-123" }) satisfies CrossLink,
      (link) => link,
      (link) => {
        expect(link.sessionId).toBe("abc-123");
        expect(link.eventId).toBeUndefined();
      },
    );
  });

  it("CrossLink with eventId", () => {
    scenario(
      () => ({ sessionId: "abc-123", eventId: "evt-456" }) satisfies CrossLink,
      (link) => link,
      (link) => {
        expect(link.sessionId).toBe("abc-123");
        expect(link.eventId).toBe("evt-456");
      },
    );
  });
});
