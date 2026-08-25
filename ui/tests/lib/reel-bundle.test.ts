import { describe, expect, it } from "vitest";
import { buildBundle, bundleText, type BundleStage } from "@/lib/reel-bundle";
import type { Reel } from "@/lib/reels-api";

const REEL: Reel = {
  id: "r1",
  title: "T",
  author: "max",
  created: "2026-08-01T00:00:00Z",
  opener: "The bottom line.",
  closer: "The end.",
  stops: [
    { line: "A spotlight.", kind: "spotlight", sessionId: "s1", eventId: "e1" },
    { line: "A diagram.", kind: "diagram", visual: { labels: ["Bash"] } },
  ],
} as Reel;

const stages = new Map<string, BundleStage>([
  ["r1:s0", { type: "snapshot", html: "<div class=\"card\">hi</div>" }],
]);

describe("buildBundle", () => {
  it("folds opener/closer into pre-resolved slides with roles and captions", () => {
    const b = buildBundle(REEL, stages, new Map(), { exportedBy: "max", now: () => "t0" });
    expect(b.v).toBe(1);
    expect(b.kind).toBe("openstory.reel-bundle");
    expect(b.reel.slides.map((s) => s.role)).toEqual(["opener", "body", "body", "closer"]);
    // title-kind slides (opener/closer render as title cards) get caption: null
    expect(b.reel.slides[0]!.caption).toBeNull();
    expect(b.reel.slides[1]!.caption).toBe("A spotlight.");
  });

  it("keeps the spotlight anchor and attaches its snapshot stage by slide id", () => {
    const b = buildBundle(REEL, stages, new Map(), { exportedBy: "max" });
    const spot = b.reel.slides.find((s) => s.kind === "spotlight")!;
    expect(spot.anchor).toEqual({ sessionId: "s1", eventId: "e1" });
    expect(spot.stage).toEqual({ type: "snapshot", html: "<div class=\"card\">hi</div>" });
  });

  it("downgrades a spotlight with no captured stage to a text stage (visible, not silent)", () => {
    const b = buildBundle(REEL, new Map(), new Map(), { exportedBy: "max" });
    const spot = b.reel.slides.find((s) => s.kind === "spotlight")!;
    expect(spot.stage).toEqual({ type: "text" });
  });

  it("carries beat ink keyed by slide id and never invents ink", () => {
    const withInk = new Map([["r1:s0", [{ type: "line", x1: 0, y1: 0, x2: 1, y2: 1 }] as const]]);
    const b = buildBundle(REEL, stages, withInk, { exportedBy: "max" });
    const inked = b.reel.slides.filter((s) => s.ink);
    expect(inked.length).toBe(1);
  });

  it("round-trips through JSON unchanged", () => {
    const b = buildBundle(REEL, stages, new Map(), { exportedBy: "max", now: () => "t0" });
    expect(JSON.parse(JSON.stringify(b))).toEqual(b);
  });
});

describe("bundleText", () => {
  it("flattens lines and snapshot text content for the scanner", () => {
    const b = buildBundle(REEL, stages, new Map(), { exportedBy: "max" });
    const rows = bundleText(b);
    expect(rows.some((r) => r.field === "line" && r.text === "A spotlight.")).toBe(true);
    expect(rows.some((r) => r.field === "snapshot" && r.text.includes("hi"))).toBe(true);
  });
});
