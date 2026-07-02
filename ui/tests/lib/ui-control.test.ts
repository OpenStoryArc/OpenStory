import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { controlToRoute, interpretControl } from "@/lib/ui-control";

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
      () => controlToRoute("open_view", { route: "heatmap" }),
      (r) => r?.view,
      (view) => expect(view).toBe("heatmap"),
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
          expect(a.route?.view).toBe("overview");
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
  it("maps query facets to a filtered Overview route", () => {
    scenario(
      () => interpretControl("query", { agent: "openactor", status: "errored", day: "2026-06-30" }),
      (a) => a,
      (a) => {
        expect(a?.type).toBe("navigate");
        if (a?.type === "navigate") {
          expect(a.route.view).toBe("overview");
          expect(a.route.overview?.filters).toEqual({ agent: "openactor", status: "errored", day: "2026-06-30" });
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
          expect(a.route.overview?.filters.search).toBe("baleen");
          expect(a.route.overview?.sort).toBe("tokens");
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

  it("coerces a numeric value and treats 'set' as an alias", () => {
    scenario(
      () => interpretControl("set", { target: "heatmap.weeks", value: 52 }),
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
