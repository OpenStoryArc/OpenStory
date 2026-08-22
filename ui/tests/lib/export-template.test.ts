import { describe, expect, it } from "vitest";
import { bakeReelHtml } from "@/lib/export-template";
import { buildBundle } from "@/lib/reel-bundle";
import type { Reel } from "@/lib/reels-api";

const REEL = {
  id: "r1", title: "My reel", author: "max", created: "c",
  opener: "Bottom line.",
  stops: [
    { line: "Spot.", kind: "spotlight", sessionId: "s1", eventId: "e1" },
    { line: "Diagram.", kind: "diagram" },
  ],
} as Reel;
const bundle = buildBundle(
  REEL,
  new Map([["s1", { type: "snapshot", html: "<div class=\"snap\">CONTENT</div>" }]]),
  new Map(),
  { exportedBy: "max", now: () => "t0" },
);

describe("bakeReelHtml", () => {
  const doc = bakeReelHtml(bundle);

  it("embeds exactly one parseable bundle JSON equal to its input", () => {
    const m = doc.match(/<script type="application\/json" id="reel-bundle">([\s\S]*?)<\/script>/g);
    expect(m?.length).toBe(1);
    const inner = m![0]!.replace(/^<script[^>]*>/, "").replace(/<\/script>$/, "");
    expect(JSON.parse(inner)).toEqual(bundle);
  });

  it("makes no external requests: no http(s) src/href, no external script/link", () => {
    expect(doc).not.toMatch(/(src|href)=["']https?:/i);
    expect(doc).not.toContain("<link rel=\"stylesheet\" href");
  });

  it("renders one section per slide and suppresses caption on title slides", () => {
    expect((doc.match(/data-slide=/g) ?? []).length).toBe(bundle.reel.slides.length);
    // opener is a title card: its section carries data-caption="" (empty)
    expect(doc).toMatch(/data-slide="s0"[^>]*data-caption=""/);
  });

  it("escapes </script> inside the embedded JSON", () => {
    // opener cleared: REEL's opener would otherwise fold into an extra title
    // slide (buildBundle, Task 1), which is orthogonal to what this case is
    // proving — that the raw </script> in a slide's line never breaks out of
    // the embedded JSON block. Isolate to exactly the one attacker-controlled slide.
    const evil = buildBundle(
      { ...REEL, opener: undefined, stops: [{ line: "x</script><script>alert(1)", kind: "title" }] } as Reel,
      new Map(), new Map(), { exportedBy: "max" },
    );
    const d = bakeReelHtml(evil);
    // the raw close tag must never appear inside the JSON block
    const inner = d.split("id=\"reel-bundle\">")[1]!.split("</script>")[0]!;
    expect(inner).not.toContain("</script>");
    expect(JSON.parse(inner.replace(/<\\\/script>/g, "</script>")).reel.slides.length).toBe(1);
  });
});
