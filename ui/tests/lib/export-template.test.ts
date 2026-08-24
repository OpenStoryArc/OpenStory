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

  it("scrubs external refs out of strokes/ink before they ever reach the baked JSON", () => {
    // The earlier "no http(s) src/href" check above only looks for HTML
    // *attribute* patterns — it never catches a URL sitting inside the
    // embedded bundle JSON, which is exactly where a stroke or ink `href`/
    // `fill` value lives. A stroke `image` href of "https://evil.example/…"
    // or a `fill: "url(https://evil/x)"` value rides along verbatim unless
    // strokes are normalized at build time — this proves the baked output
    // carries no such reference, HTML attribute or JSON string alike.
    const evilReel = {
      ...REEL,
      opener: undefined,
      stops: [{ line: "Diagram.", kind: "diagram" }],
    } as Reel;
    const evilBundle = buildBundle(
      evilReel,
      new Map([
        [
          "r1:s0",
          {
            type: "strokes",
            strokes: [
              { type: "path", points: [{ x: 0, y: 0 }, { x: 1, y: 1 }], fill: "url(https://evil.example/x)" },
            ],
          },
        ],
      ]),
      new Map([
        [
          "r1:s0",
          [{ type: "image", href: "https://evil.example/beacon.png", x: 0, y: 0, w: 0.1, h: 0.1 }],
        ],
      ]),
      { exportedBy: "max", now: () => "t0" },
    );
    const d = bakeReelHtml(evilBundle);
    // "evil.example" never appears anywhere in the document, baked JSON or
    // static player chrome alike.
    expect(d).not.toContain("://evil");
    // The embedded bundle JSON specifically must carry no http(s) reference.
    // (The static player <script> legitimately contains one benign, fixed,
    // non-fetched string — the SVG XML namespace URI passed to
    // createElementNS — so the assertion is scoped to the JSON payload
    // rather than the whole document.)
    const inner = d.split('id="reel-bundle">')[1]!.split("</script>")[0]!;
    expect(inner.toLowerCase()).not.toMatch(/https?:\/\//);
  });

  // Case-insensitive / whitespace-tolerant </script> breakout: real HTML
  // parsers close a <script> element on ANY capitalization of "script" and
  // tolerate whitespace before ">". A naive escape of only the exact
  // lowercase "</script>" leaves </SCRIPT>, </script >, and </ScRiPt>
  // untouched, letting an attacker-controlled bundle field (line, caption,
  // title, snapshot text) terminate the JSON block early and inject a real
  // DOM element (e.g. <img onerror=...>) outside it.
  describe.each([
    ["</SCRIPT>", "uppercase"],
    ["</script >", "whitespace before >"],
    ["</ScRiPt>", "mixed case"],
  ])("neutralizes %s breakout (%s)", (closeVariant, _reason) => {
    const payload = `x${closeVariant}<img src=x onerror="window.__pwned=1">`;
    const evil = buildBundle(
      { ...REEL, opener: undefined, stops: [{ line: payload, kind: "title" }] } as Reel,
      new Map(), new Map(), { exportedBy: "max" },
    );
    const d = bakeReelHtml(evil);

    it("leaves no img/script element outside the reel-bundle block when parsed as HTML", () => {
      const parsed = new DOMParser().parseFromString(d, "text/html");
      // No stray elements injected by the breakout attempt anywhere in the doc.
      expect(parsed.querySelectorAll("img").length).toBe(0);
      // Exactly the two script tags this template always emits: the JSON
      // payload and the inline player — no extra <script> from a breakout.
      expect(parsed.querySelectorAll("script").length).toBe(2);
    });

    it("still round-trips the bundle JSON, payload intact", () => {
      const parsed = new DOMParser().parseFromString(d, "text/html");
      const scriptEl = parsed.getElementById("reel-bundle");
      expect(scriptEl).not.toBeNull();
      const parsedBundle = JSON.parse(scriptEl!.textContent ?? "");
      expect(parsedBundle).toEqual(evil);
      expect(parsedBundle.reel.slides[0].line).toBe(payload);
    });
  });
});
