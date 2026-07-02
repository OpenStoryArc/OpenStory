import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { VIEW_TABS, DEFAULT_DETAIL_VIEW, type DetailView } from "@/components/explore/ExploreView";

describe("ExploreView detail tabs", () => {
  it("includes session, events, plans, graph, and search tabs", () => {
    scenario(
      () => VIEW_TABS,
      (tabs) => tabs.map((t) => t.key),
      (keys) => {
        expect(keys).toContain("session");
        expect(keys).toContain("events");
        expect(keys).toContain("plans");
        expect(keys).toContain("graph");
        expect(keys).toContain("search");
      },
    );
  });

  it("leads with the conversation-forward Session tab as the default", () => {
    scenario(
      () => ({ tabs: VIEW_TABS, def: DEFAULT_DETAIL_VIEW }),
      (x) => x,
      (x) => {
        expect(x.tabs[0]!.key).toBe("session"); // first, so it reads as the lead
        expect(x.tabs[0]!.label).toBe("Session");
        expect(x.def).toBe("session"); // opens here, not the busy Events wall
      },
    );
  });

  it("plans tab has correct label", () => {
    scenario(
      () => VIEW_TABS,
      (tabs) => tabs.find((t) => t.key === "plans"),
      (tab) => {
        expect(tab).toBeDefined();
        expect(tab!.label).toBe("Plans");
      },
    );
  });

  it("search tab has correct label", () => {
    scenario(
      () => VIEW_TABS,
      (tabs) => tabs.find((t) => t.key === "search"),
      (tab) => {
        expect(tab).toBeDefined();
        expect(tab!.label).toBe("Search");
      },
    );
  });

  it("has exactly 5 tabs including Graph", () => {
    scenario(
      () => VIEW_TABS,
      (tabs) => ({ count: tabs.length, keys: tabs.map((t) => t.key) }),
      (r) => {
        expect(r.count).toBe(5);
        expect(r.keys).toContain("graph");
      },
    );
  });

  it("DetailView type accepts 'search'", () => {
    scenario(
      () => "search" as DetailView,
      (view) => view,
      (view) => expect(view).toBe("search"),
    );
  });
});
