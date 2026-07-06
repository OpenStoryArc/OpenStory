import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { fitTransform } from "@/lib/canvas-fit";

describe("fitTransform", () => {
  it("centers the content in the viewport", () => {
    scenario(
      // symmetric bounds around origin, square viewport
      () => fitTransform({ minX: -100, maxX: 100, minY: -100, maxY: 100 }, { w: 800, h: 600 }, { pad: 0, maxScale: 10 }),
      (t) => t,
      (t) => {
        // center of bounds is (0,0) → transform should place it at viewport center
        expect(t.x).toBeCloseTo(400, 5);
        expect(t.y).toBeCloseTo(300, 5);
      },
    );
  });

  it("clamps scale to maxScale for tiny content (never over-zooms)", () => {
    scenario(
      () => fitTransform({ minX: 0, maxX: 10, minY: 0, maxY: 10 }, { w: 800, h: 600 }, { pad: 0, maxScale: 1.4 }),
      (t) => t.k,
      (k) => expect(k).toBe(1.4),
    );
  });

  it("scales down large content to fit the smaller viewport dimension", () => {
    scenario(
      // 2000x1000 content into 800x600 → limited by width: 800/2000 = 0.4 < 600/1000 = 0.6
      () => fitTransform({ minX: 0, maxX: 2000, minY: 0, maxY: 1000 }, { w: 800, h: 600 }, { pad: 0, maxScale: 1.4 }),
      (t) => t.k,
      (k) => expect(k).toBeCloseTo(0.4, 5),
    );
  });

  it("accounts for padding around the bounds", () => {
    scenario(
      // content 0..800 wide + pad 100 each side → effective 1000 wide into 1000 viewport → k=1
      () => fitTransform({ minX: 0, maxX: 800, minY: 0, maxY: 800 }, { w: 1000, h: 1000 }, { pad: 100, maxScale: 5 }),
      (t) => t.k,
      (k) => expect(k).toBeCloseTo(1, 5),
    );
  });
});
