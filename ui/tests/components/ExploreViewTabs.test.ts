import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { VIEW_TABS, type DetailView } from "@/components/explore/ExploreView";

describe("ExploreView detail tabs", () => {
  it("includes events, conversation, plans, and search tabs", () => {
    scenario(
      () => VIEW_TABS,
      (tabs) => tabs.map((t) => t.key),
      (keys) => {
        expect(keys).toContain("events");
        expect(keys).toContain("conversation");
        expect(keys).toContain("plans");
        expect(keys).toContain("search");
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

  it("has exactly 4 tabs", () => {
    scenario(
      () => VIEW_TABS,
      (tabs) => tabs.length,
      (count) => expect(count).toBe(4),
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
