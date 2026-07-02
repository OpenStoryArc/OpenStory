import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { mergeAnnotation, type Annotation } from "@/lib/annotations";

function ann(id: string, over: Partial<Annotation> = {}): Annotation {
  return { id, session_id: "s", body: "b", issuer: "i", created_at: "t", ...over };
}

describe("mergeAnnotation", () => {
  it("prepends a new annotation (newest first)", () => {
    scenario(
      () => mergeAnnotation([ann("1")], ann("2")),
      (list) => list.map((a) => a.id),
      (ids) => expect(ids).toEqual(["2", "1"]),
    );
  });

  it("de-dupes by id, keeping the newer copy at the front", () => {
    scenario(
      () => mergeAnnotation([ann("1", { body: "old" }), ann("2")], ann("1", { body: "new" })),
      (list) => list,
      (list) => {
        expect(list.map((a) => a.id)).toEqual(["1", "2"]);
        expect(list[0]!.body).toBe("new");
      },
    );
  });
});
