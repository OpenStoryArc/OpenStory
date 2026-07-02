import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { CANVAS_MODES, MODE_META, modeUsesGroupBy } from "@/lib/canvas-modes";

describe("canvas modes metadata", () => {
  it("has complete metadata (icon, label, blurb) for every mode", () => {
    scenario(
      () => CANVAS_MODES,
      (modes) => modes.map((m) => MODE_META[m]),
      (metas) => {
        for (const meta of metas) {
          expect(meta.icon.length).toBeGreaterThan(0);
          expect(meta.label.length).toBeGreaterThan(0);
          expect(meta.blurb.length).toBeGreaterThan(0);
        }
      },
    );
  });

  it("marks space/hierarchy modes as group-by-aware and scatter/flow as not", () => {
    scenario(
      () => CANVAS_MODES,
      () => ({
        grouped: (["board", "sunburst", "treemap", "gantt"] as const).map(modeUsesGroupBy),
        ungrouped: (["scatter", "flow"] as const).map(modeUsesGroupBy),
      }),
      (r) => {
        expect(r.grouped.every((x) => x === true)).toBe(true);
        expect(r.ungrouped.every((x) => x === false)).toBe(true);
      },
    );
  });

  it("includes the graduated tool-adjacency mode with complete, ungrouped metadata", () => {
    scenario(
      () => "tool-adjacency" as const,
      (m) => ({ inList: (CANVAS_MODES as readonly string[]).includes(m), meta: MODE_META[m], grouped: modeUsesGroupBy(m) }),
      ({ inList, meta, grouped }) => {
        expect(inList).toBe(true);
        expect(meta.icon.length).toBeGreaterThan(0);
        expect(meta.label.length).toBeGreaterThan(0);
        expect(meta.blurb.length).toBeGreaterThan(0);
        expect(grouped).toBe(false); // every session's tools, not group-by
        expect(meta.groupByNote && meta.groupByNote.length).toBeTruthy();
      },
    );
  });

  it("gives every non-group-by mode a note explaining the absence", () => {
    scenario(
      () => CANVAS_MODES.filter((m) => !modeUsesGroupBy(m)),
      (modes) => modes.map((m) => MODE_META[m].groupByNote),
      (notes) => {
        expect(notes.length).toBeGreaterThan(0);
        for (const n of notes) expect(n && n.length).toBeTruthy();
      },
    );
  });
});
