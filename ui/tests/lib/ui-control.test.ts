import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { controlToRoute, interpretControl } from "@/lib/ui-control";
import { planNavigateTo } from "@/lib/nav-path";

describe("draw — agent pen", () => {
  it("interprets draw with strokes and clear", () => {
    const a = interpretControl("draw", {
      clear: true,
      label: "smiley",
      strokes: [{ type: "circle", cx: 0.5, cy: 0.5, r: 0.1 }],
    });
    expect(a).toMatchObject({ type: "draw", clear: true, label: "smiley" });
    if (a?.type === "draw") expect(a.strokes).toHaveLength(1);
  });

  it("maps set draw.clear to an empty scene", () => {
    const a = interpretControl("set", { target: "draw.clear" });
    expect(a).toMatchObject({ type: "draw", clear: true });
  });
});


describe("controlToRoute", () => {
  it("resolves open_view with a hash route to a parsed route", () => {
    scenario(
      () => controlToRoute("open_view", { route: "#/explore/abc123" }),
      (r) => r,
      (r) => {
        expect(r?.view).toBe("explore");
        expect(r?.sessionId).toBe("abc123");
      },
    );
  });

  it("tolerates a route missing the leading # or /", () => {
    scenario(
      // legacy "heatmap" now aliases onto Canvas (the heatmap is a mode there)
      () => controlToRoute("open_view", { route: "heatmap" }),
      (r) => r?.view,
      (view) => expect(view).toBe("canvas"),
    );
  });

  it("resolves a structured { view, sessionId } intent", () => {
    scenario(
      () => controlToRoute("open_view", { view: "story", sessionId: "s-9" }),
      (r) => r,
      (r) => {
        expect(r?.view).toBe("story");
        expect(r?.sessionId).toBe("s-9");
      },
    );
  });

  it("carries searchQuery so an agent can drive cross-session search (file→session)", () => {
    // The file→session canopy edge: impact-across-sessions is the FTS
    // search surface; the seam reaches it via open_view + searchQuery.
    scenario(
      () =>
        controlToRoute("open_view", {
          view: "explore",
          detailView: "search",
          searchQuery: "src/auth.rs",
        }),
      (r) => r,
      (r) => {
        expect(r?.view).toBe("explore");
        expect(r?.detailView).toBe("search");
        expect(r?.searchQuery).toBe("src/auth.rs");
      },
    );
  });

  it("carries eventId into the route so replay retraces to the exact event", () => {
    scenario(
      () => controlToRoute("open_view", { view: "explore", sessionId: "s-9", eventId: "e-42" }),
      (r) => r,
      (r) => {
        expect(r?.view).toBe("explore");
        expect(r?.sessionId).toBe("s-9");
        expect(r?.eventId).toBe("e-42");
      },
    );
  });

  it("carries reelId + reelAutoplay so a planned reel navigate_to round-trips through open_view", () => {
    // FINDING 3: planNavigateTo({kind:"reel", ...}) emits
    // {action:"open_view", params:{view:"reels", reelId, autoplay}} (see
    // nav-path.ts's reel branch), but controlToRoute previously only
    // carried sessionId/detailView/eventId/etc — every planned-step
    // consumer (runControlSequence fallback, foldSteps in realizeIntent,
    // raw ui_control open_view calls) landed on the reels LIST, silently
    // dropping which reel and whether to autoplay it.
    scenario(
      () => {
        const steps = planNavigateTo({ kind: "reel", id: "reel-abc", autoplay: true });
        return steps?.[0]?.params;
      },
      (params) => controlToRoute("open_view", params),
      (r) => {
        expect(r?.view).toBe("reels");
        expect(r?.reelId).toBe("reel-abc");
        expect(r?.reelAutoplay).toBe(true);
      },
    );
  });

  it("returns null for non-navigation actions", () => {
    scenario(
      () => controlToRoute("highlight", { sessionIds: ["a"] }),
      (r) => r,
      (r) => expect(r).toBeNull(),
    );
  });

  it("returns null for a malformed open_view (no route or view)", () => {
    scenario(
      () => controlToRoute("open_view", { foo: "bar" }),
      (r) => r,
      (r) => expect(r).toBeNull(),
    );
  });
});

describe("interpretControl (control vocabulary)", () => {
  it("maps open_view to a navigate action", () => {
    scenario(
      () => interpretControl("open_view", { route: "#/story/s1" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("navigate");
        if (a?.type === "navigate") expect(a.route.sessionId).toBe("s1");
      },
    );
  });

  it("maps present (message + sessionIds + route) to a present action", () => {
    scenario(
      () => interpretControl("present", { message: "look at these failures", sessionIds: ["a", 3, "b"], route: "#/overview" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("present");
        if (a?.type === "present") {
          expect(a.message).toBe("look at these failures");
          expect(a.sessionIds).toEqual(["a", "b"]); // non-strings filtered
          // "#/overview" is the living legacy-alias proof — it resolves to Explore.
          expect(a.route?.view).toBe("explore");
        }
      },
    );
  });

  it("treats highlight/announce as present aliases", () => {
    scenario(
      () => ({
        hi: interpretControl("highlight", { sessionIds: ["x"] }),
        an: interpretControl("announce", { note: "heads up" }),
      }),
      (r) => r,
      (r) => {
        expect(r.hi?.type).toBe("present");
        expect(r.an?.type).toBe("present");
        if (r.an?.type === "present") expect(r.an.message).toBe("heads up");
      },
    );
  });

  it("returns null for an empty present and unknown actions", () => {
    scenario(
      () => ({ empty: interpretControl("present", {}), unknown: interpretControl("frobnicate", { x: 1 }) }),
      (r) => r,
      (r) => {
        expect(r.empty).toBeNull();
        expect(r.unknown).toBeNull();
      },
    );
  });
});

describe("interpretControl — query class", () => {
  it("maps query facets to a filtered Explore route", () => {
    scenario(
      () => interpretControl("query", { agent: "openactor", status: "errored", day: "2026-06-30" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("navigate");
        if (a?.type === "navigate") {
          expect(a.route.view).toBe("explore");
          expect(a.route.explore?.filters).toEqual({ agent: "openactor", status: "errored", day: "2026-06-30" });
        }
      },
    );
  });

  it("accepts a free-text search (q alias) and a sort", () => {
    scenario(
      () => interpretControl("filter", { q: "baleen", sort: "tokens" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("navigate");
        if (a?.type === "navigate") {
          expect(a.route.explore?.filters.search).toBe("baleen");
          expect(a.route.explore?.sort).toBe("tokens");
        }
      },
    );
  });

  it("ignores unknown keys / bad sort and returns null when nothing narrows", () => {
    scenario(
      () => ({ empty: interpretControl("query", { nonsense: "x", sort: "bogus" }), none: interpretControl("query", {}) }),
      (r) => r,
      (r) => {
        expect(r.empty).toBeNull();
        expect(r.none).toBeNull();
      },
    );
  });
});

describe("interpretControl — toggle class", () => {
  it("maps toggle{target,value} to a toggle action (value coerced to string)", () => {
    scenario(
      () => interpretControl("toggle", { target: "canvas.mode", value: "sunburst" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("toggle");
        if (a?.type === "toggle") { expect(a.target).toBe("canvas.mode"); expect(a.value).toBe("sunburst"); }
      },
    );
  });

  it("coerces a numeric toggle value to a string", () => {
    scenario(
      () => interpretControl("toggle", { target: "heatmap.weeks", value: 52 }),
      (a) => a,
      (a) => { if (a?.type === "toggle") { expect(a.target).toBe("heatmap.weeks"); expect(a.value).toBe("52"); } else expect.fail("not toggle"); },
    );
  });

  it("returns null without a target or value", () => {
    scenario(
      () => ({ noTarget: interpretControl("toggle", { value: "x" }), noValue: interpretControl("toggle", { target: "canvas.mode" }) }),
      (r) => r,
      (r) => { expect(r.noTarget).toBeNull(); expect(r.noValue).toBeNull(); },
    );
  });
});

describe("interpretControl — set (structured) action", () => {
  it("maps set{target, ...fields} to a set action with the object payload", () => {
    scenario(
      () => interpretControl("set", { target: "scatter.brush", ev0: 10, ev1: 100, tok0: 1000, tok1: 50000 }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("set");
        if (a?.type === "set") {
          expect(a.target).toBe("scatter.brush");
          expect(a.params).toEqual({ ev0: 10, ev1: 100, tok0: 1000, tok1: 50000 });
        }
      },
    );
  });

  it("returns null without a target", () => {
    scenario(
      () => interpretControl("set", { ev0: 1 }),
      (a) => a,
      (a) => expect(a).toBeNull(),
    );
  });
});

describe("interpretControl — navigate_to (high-level hand)", () => {
  it("plans event focus as navigate_sequence", () => {
    const a = interpretControl("navigate_to", {
      kind: "event",
      id: "EVT-1",
      sessionId: "SES-1",
      details: true,
    });
    expect(a?.type).toBe("navigate_sequence");
    if (a?.type === "navigate_sequence") {
      expect(a.steps.length).toBeGreaterThanOrEqual(2);
      expect(a.steps[0]?.action).toBe("focus_event");
    }
  });

  it("plans canvas mode + session select", () => {
    const a = interpretControl("navigate_to", {
      kind: "session",
      id: "SES-1",
      canvasMode: "gantt",
    });
    expect(a?.type).toBe("navigate_sequence");
    if (a?.type === "navigate_sequence") {
      expect(a.steps.some((s) => s.params.value === "gantt")).toBe(true);
      expect(a.steps.some((s) => s.params.target === "canvas.select_session")).toBe(true);
    }
  });
});

describe("interpretControl — focus_event (navigate-to-thing)", () => {
  it("focuses an event in Explore by default", () => {
    scenario(
      () => interpretControl("focus_event", { sessionId: "s1", eventId: "e9" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("navigate");
        if (a?.type === "navigate") {
          expect(a.route.view).toBe("explore");
          expect(a.route.sessionId).toBe("s1");
          expect(a.route.eventId).toBe("e9");
        }
      },
    );
  });

  it("targets Story when view:'story'", () => {
    scenario(
      () => interpretControl("focus_event", { sessionId: "s1", eventId: "e9", view: "story" }),
      (a) => (a?.type === "navigate" ? a.route.view : null),
      (v) => expect(v).toBe("story"),
    );
  });

  it("returns null without both sessionId and eventId", () => {
    scenario(
      () => [interpretControl("focus_event", { sessionId: "s1" }), interpretControl("focus_event", { eventId: "e9" })],
      (r) => r,
      ([a, b]) => { expect(a).toBeNull(); expect(b).toBeNull(); },
    );
  });
});

describe("interpretControl — spotlight (presentation mode)", () => {
  it("focus_event with spotlight:true becomes a spotlight action, not a navigation", () => {
    scenario(
      () => interpretControl("focus_event", { sessionId: "s1", eventId: "e9", spotlight: true }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("spotlight");
        if (a?.type === "spotlight") {
          expect(a.sessionId).toBe("s1");
          expect(a.eventId).toBe("e9");
        }
      },
    );
  });

  it("only a literal true upgrades to spotlight — falsy/other values still navigate", () => {
    scenario(
      () => ({
        off: interpretControl("focus_event", { sessionId: "s1", eventId: "e9", spotlight: false }),
        stringy: interpretControl("focus_event", { sessionId: "s1", eventId: "e9", spotlight: "yes" }),
      }),
      (r) => r,
      (r) => {
        expect(r.off?.type).toBe("navigate");
        expect(r.stringy?.type).toBe("navigate");
      },
    );
  });

  it("spotlight still requires both sessionId and eventId", () => {
    scenario(
      () => interpretControl("focus_event", { eventId: "e9", spotlight: true }),
      (a) => a,
      (a) => expect(a).toBeNull(),
    );
  });

  it("toggle {target:'spotlight', value:'off'} parses as the dismissal seam", () => {
    scenario(
      () => interpretControl("toggle", { target: "spotlight", value: "off" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("toggle");
        if (a?.type === "toggle") {
          expect(a.target).toBe("spotlight");
          expect(a.value).toBe("off");
        }
      },
    );
  });

  it("spotlight carries clipAt (shot framing); blank/missing clipAt is undefined", () => {
    scenario(
      () => ({
        clipped: interpretControl("focus_event", {
          sessionId: "s1",
          eventId: "e9",
          spotlight: true,
          clipAt: "Where that leaves us",
        }),
        blank: interpretControl("focus_event", { sessionId: "s1", eventId: "e9", spotlight: true, clipAt: "  " }),
        absent: interpretControl("focus_event", { sessionId: "s1", eventId: "e9", spotlight: true }),
      }),
      (r) => r,
      (r) => {
        expect(r.clipped?.type).toBe("spotlight");
        if (r.clipped?.type === "spotlight") expect(r.clipped.clipAt).toBe("Where that leaves us");
        if (r.blank?.type === "spotlight") expect(r.blank.clipAt).toBeUndefined();
        if (r.absent?.type === "spotlight") expect(r.absent.clipAt).toBeUndefined();
      },
    );
  });

  it("present with spotlight:true becomes a full-screen title card", () => {
    scenario(
      () => ({
        title: interpretControl("present", { message: "Have your agent read your agent history to you.", spotlight: true }),
        banner: interpretControl("present", { message: "hello", spotlight: false }),
        empty: interpretControl("present", { message: "   ", spotlight: true }),
      }),
      (r) => r,
      (r) => {
        expect(r.title?.type).toBe("title");
        if (r.title?.type === "title") {
          expect(r.title.message).toBe("Have your agent read your agent history to you.");
        }
        expect(r.banner?.type).toBe("present");
        expect(r.empty).toBeNull();
      },
    );
  });
});

describe("when agent opens any hash-routable state", () => {
  it("should produce HashRoute with filePath from structured open_view params", () => {
    scenario(
      () =>
        controlToRoute("open_view", {
          view: "explore",
          sessionId: "s1",
          filePath: "src/lib/ui-control.ts",
        }),
      (r) => r,
      (r) => {
        expect(r?.view).toBe("explore");
        expect(r?.sessionId).toBe("s1");
        expect(r?.filePath).toBe("src/lib/ui-control.ts");
      },
    );
  });

  it("should carry Live userFilter and timeFilter via structured params", () => {
    scenario(
      () =>
        controlToRoute("open_view", {
          view: "live",
          sessionId: "s1",
          userFilter: "katie",
          timeFilter: "today",
        }),
      (r) => r,
      (r) => {
        expect(r?.view).toBe("live");
        expect(r?.userFilter).toBe("katie");
        expect(r?.timeFilter).toBe("today");
      },
    );
  });

  it("should carry explore facet filters + sort on open_view", () => {
    scenario(
      () =>
        controlToRoute("open_view", {
          view: "explore",
          agent: "grok",
          project: "OpenStory",
          sort: "tokens",
          range: "7d",
        }),
      (r) => r,
      (r) => {
        expect(r?.view).toBe("explore");
        expect(r?.explore?.filters).toEqual({ agent: "grok", project: "OpenStory", range: "7d" });
        expect(r?.explore?.sort).toBe("tokens");
      },
    );
  });

  it("should open explore conversation detailView via structured params", () => {
    scenario(
      () =>
        interpretControl("open_view", {
          view: "explore",
          sessionId: "SES",
          detailView: "conversation",
        }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("navigate");
        if (a?.type === "navigate") {
          expect(a.route.detailView).toBe("conversation");
          expect(a.route.sessionId).toBe("SES");
        }
      },
    );
  });

  it("should treat a full hash string as an escape hatch for any bookmarkable state", () => {
    scenario(
      () =>
        interpretControl("open_view", {
          route: "#/explore/SES/conversation?agent=grok&sort=recent",
        }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("navigate");
        if (a?.type === "navigate") {
          expect(a.route.view).toBe("explore");
          expect(a.route.sessionId).toBe("SES");
          expect(a.route.detailView).toBe("conversation");
          expect(a.route.explore?.filters.agent).toBe("grok");
          expect(a.route.explore?.sort).toBe("recent");
        }
      },
    );
  });

  it("should drop invalid timeFilter values silently", () => {
    scenario(
      () => controlToRoute("open_view", { view: "live", timeFilter: "bogus" }),
      (r) => r?.timeFilter,
      (t) => expect(t).toBeUndefined(),
    );
  });
});

describe("when agent runs a storytelling tour sequence", () => {
  it("should map open_view story → present → focus_event spotlight → query → explore conversation", () => {
    scenario(
      () => [
        interpretControl("open_view", { view: "story", sessionId: "tour-s1" }),
        interpretControl("present", {
          message: "Here is the arc of this session",
          spotlight: true,
        }),
        interpretControl("focus_event", {
          sessionId: "tour-s1",
          eventId: "evt-peak",
          spotlight: true,
        }),
        interpretControl("query", { agent: "grok" }),
        interpretControl("open_view", {
          view: "explore",
          sessionId: "tour-s1",
          detailView: "conversation",
        }),
      ],
      (steps) => steps,
      (steps) => {
        expect(steps[0]?.type).toBe("navigate");
        if (steps[0]?.type === "navigate") {
          expect(steps[0].route).toEqual({ view: "story", sessionId: "tour-s1" });
        }
        expect(steps[1]?.type).toBe("title");
        if (steps[1]?.type === "title") {
          expect(steps[1].message).toBe("Here is the arc of this session");
        }
        expect(steps[2]?.type).toBe("spotlight");
        if (steps[2]?.type === "spotlight") {
          expect(steps[2].sessionId).toBe("tour-s1");
          expect(steps[2].eventId).toBe("evt-peak");
        }
        expect(steps[3]?.type).toBe("navigate");
        if (steps[3]?.type === "navigate") {
          expect(steps[3].route.explore?.filters.agent).toBe("grok");
        }
        expect(steps[4]?.type).toBe("navigate");
        if (steps[4]?.type === "navigate") {
          expect(steps[4].route.detailView).toBe("conversation");
        }
      },
    );
  });
});

describe("when agent queries with a range window", () => {
  it("should include range in explore filters", () => {
    scenario(
      () => interpretControl("query", { agent: "claude-code", range: "30d", sort: "events" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("navigate");
        if (a?.type === "navigate") {
          expect(a.route.explore?.filters.range).toBe("30d");
          expect(a.route.explore?.filters.agent).toBe("claude-code");
          expect(a.route.explore?.sort).toBe("events");
        }
      },
    );
  });
});
