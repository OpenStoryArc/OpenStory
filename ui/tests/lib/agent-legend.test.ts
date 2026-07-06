import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { agentsPresent } from "@/components/canvas/AgentLegend";

describe("agentsPresent", () => {
  it("orders agents by frequency descending, then name", () => {
    scenario(
      () => ["pi-mono", "claude-code", "claude-code", "claude-code", "pi-mono"],
      (list) => agentsPresent(list),
      (out) => expect(out).toEqual(["claude-code", "pi-mono"]),
    );
  });

  it("dedupes and maps blanks to 'unknown'", () => {
    scenario(
      () => ["claude-code", "", "claude-code"],
      (list) => agentsPresent(list),
      (out) => {
        expect(out).toContain("claude-code");
        expect(out).toContain("unknown");
        expect(out).toHaveLength(2);
      },
    );
  });

  it("returns empty for no agents", () => {
    scenario(
      () => [],
      (list) => agentsPresent(list),
      (out) => expect(out).toEqual([]),
    );
  });
});
