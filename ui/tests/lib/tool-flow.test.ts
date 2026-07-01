import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { countTransitions, buildToolFlow } from "@/lib/tool-flow";

describe("countTransitions", () => {
  it("counts consecutive tool pairs across sessions", () => {
    scenario(
      () => countTransitions([["Bash", "Bash", "Edit"], ["Bash", "Edit"]]),
      (m) => Object.fromEntries(m),
      (o) => {
        expect(o["Bash|Bash"]).toBe(1);
        expect(o["Bash|Edit"]).toBe(2);
        expect(o["Edit|Bash"]).toBeUndefined();
      },
    );
  });
});

describe("buildToolFlow", () => {
  it("builds bipartite from/to nodes whose values equal their link sums", () => {
    scenario(
      () => buildToolFlow([["Bash", "Edit", "Bash", "Edit"]], { height: 100, minCount: 1 }),
      (f) => f,
      (f) => {
        // transitions: Bash->Edit x2, Edit->Bash x1
        expect(f.total).toBe(3);
        const fromBash = f.fromNodes.find((n) => n.tool === "Bash")!;
        expect(fromBash.value).toBe(2); // Bash->Edit twice
        const toBash = f.toNodes.find((n) => n.tool === "Bash")!;
        expect(toBash.value).toBe(1); // Edit->Bash once
        // node heights are proportional and within the canvas
        expect(f.fromNodes.every((n) => n.y1 <= 100.001)).toBe(true);
      },
    );
  });

  it("thresholds rare transitions and folds the tail into 'other'", () => {
    scenario(
      () => buildToolFlow(
        [["A", "A", "A", "A", "A", "B", "B", "B", "C", "D", "E", "F", "G", "H", "I"]],
        { height: 200, minCount: 1, topN: 3 },
      ),
      (f) => f,
      (f) => {
        // only top-3 tools survive as named nodes; the rest fold to "other"
        const names = new Set([...f.fromNodes, ...f.toNodes].map((n) => n.tool));
        expect([...names].every((n) => ["A", "B", "C", "other"].includes(n))).toBe(true);
        expect(names.has("other")).toBe(true);
      },
    );
  });

  it("returns an empty flow when there are no transitions", () => {
    expect(buildToolFlow([["Bash"]], { height: 100 }).total).toBe(0);
  });
});
