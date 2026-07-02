import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { buildAdjacencyMatrix } from "@/lib/tool-adjacency";

describe("buildAdjacencyMatrix", () => {
  it("counts from→to transitions into cells with a max", () => {
    scenario(
      () => buildAdjacencyMatrix([["Read", "Edit", "Bash"], ["Read", "Edit"]]),
      (m) => m,
      (m) => {
        expect(m.tools).toContain("Read");
        expect(m.tools).toContain("Edit");
        const readEdit = m.cells.find((c) => c.from === "Read" && c.to === "Edit");
        expect(readEdit?.count).toBe(2); // Read→Edit appears twice
        expect(m.max).toBe(2);
        expect(m.total).toBe(3); // Read→Edit, Edit→Bash, Read→Edit
      },
    );
  });

  it("keeps only the top-N tools and drops transitions touching cut tools", () => {
    scenario(
      // A↔B dominate; Z appears once → excluded at topN=2
      () => buildAdjacencyMatrix([["A", "B", "A", "B", "A", "B", "Z"]], { topN: 2 }),
      (m) => m,
      (m) => {
        expect(m.tools.sort()).toEqual(["A", "B"]);
        expect(m.cells.some((c) => c.from === "Z" || c.to === "Z")).toBe(false);
      },
    );
  });

  it("is empty for sequences with no transitions", () => {
    scenario(
      () => buildAdjacencyMatrix([["OnlyOne"], []]),
      (m) => m,
      (m) => { expect(m.cells).toHaveLength(0); expect(m.total).toBe(0); },
    );
  });
});
