import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { controlToRoute } from "@/lib/ui-control";

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
