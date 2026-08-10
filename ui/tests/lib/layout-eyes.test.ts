import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  collectLayoutEyes,
  layoutEyesToWire,
  normalizeDomRect,
  padLayoutRect,
  pickFocusTarget,
  ringStrokesForRect,
  type LayoutTarget,
} from "@/lib/layout-eyes";

describe("normalizeDomRect", () => {
  it("maps pixel box into unit viewport space", () => {
    scenario(
      () =>
        normalizeDomRect(
          { left: 100, top: 50, width: 200, height: 100 },
          { w: 1000, h: 500 },
        ),
      (r) => r,
      (r) => {
        expect(r!.x).toBeCloseTo(0.1);
        expect(r!.y).toBeCloseTo(0.1);
        expect(r!.w).toBeCloseTo(0.2);
        expect(r!.h).toBeCloseTo(0.2);
      },
    );
  });

  it("returns null for zero viewport or zero size", () => {
    expect(
      normalizeDomRect({ left: 0, top: 0, width: 10, height: 10 }, { w: 0, h: 100 }),
    ).toBeNull();
    expect(
      normalizeDomRect({ left: 0, top: 0, width: 0, height: 10 }, { w: 100, h: 100 }),
    ).toBeNull();
  });

  it("clamps off-screen overhang into 0..1", () => {
    const r = normalizeDomRect(
      { left: -50, top: -20, width: 200, height: 100 },
      { w: 100, h: 100 },
    );
    expect(r).not.toBeNull();
    expect(r!.x).toBe(0);
    expect(r!.y).toBe(0);
    expect(r!.w).toBe(1);
    expect(r!.h).toBe(0.8);
  });
});

describe("pickFocusTarget", () => {
  const targets: LayoutTarget[] = [
    { kind: "session", id: "s1", rect: { x: 0, y: 0, w: 0.2, h: 0.5 } },
    { kind: "event", id: "e9", rect: { x: 0.3, y: 0.2, w: 0.4, h: 0.1 } },
    { kind: "spotlight", id: "e9", rect: { x: 0.1, y: 0.1, w: 0.8, h: 0.8 } },
  ];

  it("prefers explicit id", () => {
    expect(pickFocusTarget(targets, { id: "e9" })?.kind).toBe("spotlight");
  });

  it("ranks spotlight > event > session when no prefer", () => {
    expect(pickFocusTarget(targets)?.kind).toBe("spotlight");
  });

  it("returns null for empty", () => {
    expect(pickFocusTarget([])).toBeNull();
  });
});

describe("ringStrokesForRect", () => {
  it("emits a closed path ring (and optional label)", () => {
    scenario(
      () =>
        ringStrokesForRect(
          { x: 0.2, y: 0.3, w: 0.4, h: 0.2 },
          { label: "event e9", pad: 0.01 },
        ),
      (s) => s,
      (s) => {
        expect(s.length).toBe(2);
        expect(s[0]).toMatchObject({ type: "path", closed: true, fill: "none" });
        if (s[0]?.type === "path") {
          expect(s[0].points).toHaveLength(4);
          // pad expands outward
          expect(s[0].points[0]!.x).toBeLessThan(0.2);
        }
        expect(s[1]).toMatchObject({ type: "text", text: "event e9" });
      },
    );
  });
});

describe("padLayoutRect", () => {
  it("grows then clamps", () => {
    const r = padLayoutRect({ x: 0, y: 0, w: 1, h: 1 }, 0.05);
    expect(r).toEqual({ x: 0, y: 0, w: 1, h: 1 });
  });
});

describe("collectLayoutEyes (DOM)", () => {
  it("reads data-os-target nodes and picks focus by preferId", () => {
    const root = document.createElement("div");
    root.innerHTML = `
      <div data-os-target="session" data-os-id="s1" style="position:fixed;left:0;top:0;width:100px;height:200px"></div>
      <div data-os-target="event" data-os-id="e9" style="position:fixed;left:200px;top:100px;width:300px;height:80px"></div>
    `;
    document.body.appendChild(root);
    // jsdom getBoundingClientRect is often 0 — stub
    const els = root.querySelectorAll("[data-os-target]");
    els[0]!.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 100, height: 200, right: 100, bottom: 200, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
    els[1]!.getBoundingClientRect = () =>
      ({
        left: 200,
        top: 100,
        width: 300,
        height: 80,
        right: 500,
        bottom: 180,
        x: 200,
        y: 100,
        toJSON: () => ({}),
      }) as DOMRect;

    const eyes = collectLayoutEyes({
      root,
      preferId: "e9",
      viewport: { w: 1000, h: 1000 },
      now: () => "2026-08-08T00:00:00.000Z",
    });
    expect(eyes.targets).toHaveLength(2);
    expect(eyes.focus?.id).toBe("e9");
    expect(eyes.focus?.kind).toBe("event");
    expect(eyes.focus?.rect.x).toBeCloseTo(0.2);
    expect(eyes.focus?.rect.w).toBeCloseTo(0.3);

    const wire = layoutEyesToWire(eyes);
    expect(wire.focus?.id).toBe("e9");
    expect(wire.at).toBe("2026-08-08T00:00:00.000Z");

    document.body.removeChild(root);
  });

  it("falls back to data-event-id legacy markers", () => {
    const root = document.createElement("div");
    const el = document.createElement("div");
    el.setAttribute("data-event-id", "legacy-e");
    el.getBoundingClientRect = () =>
      ({ left: 10, top: 10, width: 50, height: 50, right: 60, bottom: 60, x: 10, y: 10, toJSON: () => ({}) }) as DOMRect;
    root.appendChild(el);
    document.body.appendChild(root);

    const eyes = collectLayoutEyes({ root, viewport: { w: 100, h: 100 } });
    expect(eyes.targets[0]).toMatchObject({ kind: "event", id: "legacy-e" });
    document.body.removeChild(root);
  });
});
