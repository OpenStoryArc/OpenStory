/** The file→session canopy edge, human side: a SELECTED file facet offers
 *  "↺ impact across sessions" — a link to the cross-session FTS search for
 *  that path. Unselected facets stay calm (detail on click). */

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { FacetPanel } from "@/components/explore/FacetPanel";
import { buildHash } from "@/lib/hash-route";

const FILES = [{ path: "/home/max/src/auth.rs", count: 5, reads: 3, writes: 2 }];

function renderPanel(selectedFile: string | null) {
  return render(
    <FacetPanel
      files={FILES}
      tools={[]}
      plans={[]}
      selectedFile={selectedFile}
      selectedTool={null}
      selectedPlan={null}
      onSelectFile={() => {}}
      onSelectTool={() => {}}
      onSelectPlan={() => {}}
    />,
  );
}

describe("when a file facet is selected", () => {
  it("should link to that file's impact across sessions (FTS search)", () => {
    renderPanel("/home/max/src/auth.rs");
    const link = screen.getByTestId("file-impact-link");
    // The canonical search hash for this path — one builder, no drift.
    expect(link.getAttribute("href")).toBe(
      buildHash({ view: "explore", detailView: "search", searchQuery: "/home/max/src/auth.rs" }),
    );
  });
});

describe("when no file is selected", () => {
  it("should keep the default calm — no impact link", () => {
    renderPanel(null);
    expect(screen.queryByTestId("file-impact-link")).toBeNull();
  });
});
