/** Switching tabs must carry the session you're looking at — Live→Story
 *  keeps the session instead of dumping you on an empty Story view.
 *  (P2 canopy: carry-session-across-tabs — the first navigation dead-end.) */

import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { switchTabRoute } from "@/lib/navigation";
import type { HashRoute } from "@/lib/hash-route";

describe("when switching tabs while a session is selected", () => {
  it("should carry the session into every session-capable view", () =>
    scenario(
      () => ({ view: "live", sessionId: "sess-1" }) as HashRoute,
      (route) => ({
        story: switchTabRoute(route, "story"),
        explore: switchTabRoute(route, "explore"),
        canvas: switchTabRoute(route, "canvas"),
      }),
      ({ story, explore, canvas }) => {
        expect(story).toEqual({ view: "story", sessionId: "sess-1" });
        expect(explore).toEqual({ view: "explore", sessionId: "sess-1" });
        expect(canvas).toEqual({ view: "canvas", sessionId: "sess-1" });
      },
    ));

  it("should drop the session for views without a session context", () =>
    scenario(
      () => ({ view: "live", sessionId: "sess-1" }) as HashRoute,
      (route) => ({
        storm: switchTabRoute(route, "storm"),
        admin: switchTabRoute(route, "admin"),
      }),
      ({ storm, admin }) => {
        expect(storm).toEqual({ view: "storm" });
        expect(admin).toEqual({ view: "admin" });
      },
    ));
});

describe("when no session is selected", () => {
  it("should switch cleanly with no session id", () =>
    scenario(
      () => ({ view: "live" }) as HashRoute,
      (route) => switchTabRoute(route, "story"),
      (next) => expect(next).toEqual({ view: "story" }),
    ));
});

describe("when leaving a Story session for Live", () => {
  it("should carry the session back the other way too", () =>
    scenario(
      () => ({ view: "story", sessionId: "sess-9" }) as HashRoute,
      (route) => switchTabRoute(route, "live"),
      (next) => expect(next).toEqual({ view: "live", sessionId: "sess-9" }),
    ));
});
