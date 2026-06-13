import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  rankField,
  sumChangeField,
  countByType,
  type ShapeRow,
} from "@/lib/shape-detail";

function row(shape_type: string, data: Record<string, unknown>, seq = 0): ShapeRow {
  return {
    id: `e${seq}:${shape_type}:0`,
    session_id: "s",
    shape_type,
    seq,
    timestamp: "2026-01-01T00:00:00Z",
    event_id: `e${seq}`,
    data,
  };
}

describe("shape-detail transforms", () => {
  it("ranks a scalar field descending, ties broken by value asc", () => {
    scenario(
      () => [
        row("bash-shape", { program: "git" }),
        row("bash-shape", { program: "git" }),
        row("bash-shape", { program: "cargo" }),
        row("path-shape", { top_segment: "rs" }), // wrong type — ignored
      ],
      (rows) => rankField(rows, "bash-shape", "program"),
      (ranked) => {
        expect(ranked).toEqual([
          { value: "git", count: 2 },
          { value: "cargo", count: 1 },
        ]);
      },
    );
  });

  it("unnests array fields like naming_tokens", () => {
    scenario(
      () => [
        row("path-shape", { naming_tokens: ["event", "store"] }),
        row("path-shape", { naming_tokens: ["store", "sqlite"] }),
      ],
      (rows) => rankField(rows, "path-shape", "naming_tokens"),
      (ranked) => {
        expect(ranked[0]).toEqual({ value: "store", count: 2 });
        expect(ranked.map((r) => r.value)).toContain("event");
        expect(ranked.map((r) => r.value)).toContain("sqlite");
      },
    );
  });

  it("skips empty values and respects the limit", () => {
    scenario(
      () => [
        row("bash-shape", { subcommand: "" }),
        row("bash-shape", { subcommand: "status" }),
      ],
      (rows) => rankField(rows, "bash-shape", "subcommand", 1),
      (ranked) => {
        expect(ranked).toEqual([{ value: "status", count: 1 }]);
      },
    );
  });

  it("sums numeric change fields only over change-shape rows", () => {
    scenario(
      () => [
        row("change-shape", { lines_added: 10 }),
        row("change-shape", { lines_added: 5 }),
        row("bash-shape", { lines_added: 999 }), // wrong type — ignored
      ],
      (rows) => sumChangeField(rows, "lines_added"),
      (total) => expect(total).toBe(15),
    );
  });

  it("counts rows by shape type", () => {
    scenario(
      () => [row("bash-shape", {}), row("bash-shape", {}), row("path-shape", {})],
      (rows) => [countByType(rows, "bash-shape"), countByType(rows, "path-shape")],
      ([bash, path]) => {
        expect(bash).toBe(2);
        expect(path).toBe(1);
      },
    );
  });
});
