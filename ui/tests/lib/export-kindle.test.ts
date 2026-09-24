/** export-kindle — the Kindle reading edition, rendered from a ReelBundle.
 *
 *  Second renderer of the bundle (the interactive `.reel.html` is the first).
 *  A reading edition must survive Send to Kindle: no script, no external
 *  assets, raster data-URL images only, prose for everything else, and the
 *  scan receipt riding along as a comment since KF8 strips script blocks.
 *  Spec: docs/superpowers/specs/2026-09-23-reel-export-kindle-default-design.md */

import { describe, expect, it } from "vitest";
import { bakeKindleHtml } from "@/lib/export-kindle";
import type { ReelBundle } from "@/lib/reel-bundle";

const PNG = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

function bundle(over: Partial<ReelBundle> = {}): ReelBundle {
  return {
    v: 1,
    kind: "openstory.reel-bundle",
    exportedAt: "2026-09-23T20:00:00.000Z",
    exportedBy: "Max",
    reel: {
      id: "reel-1",
      title: "The arc <of> OpenStory",
      author: "claude",
      created: "2026-09-23T19:00:00Z",
      slides: [
        { id: "opener", kind: "title", role: "opener", line: "Three months, one pattern.", caption: null, stage: { type: "text" } },
        {
          id: "s1",
          kind: "image",
          role: "body",
          line: "Here is the shape of the quarter.",
          caption: "Here is the shape of the quarter.",
          title: "Commits on master, by week",
          stage: { type: "image", dataUri: PNG },
        },
        {
          id: "s2",
          kind: "spotlight",
          role: "body",
          line: "Every day the fleet grew.",
          caption: "Every day the fleet grew.",
          anchor: { sessionId: "917baaad-5bf7", eventId: "73343c69-38f9" },
          stage: { type: "snapshot", html: "<div><p>tell me more about openstory</p><p>it takes 4G of RAM?</p></div>" },
        },
        {
          id: "s3",
          kind: "diagram",
          role: "body",
          line: "Three layers.",
          caption: "Three layers.",
          title: "The stream and the two maps",
          stage: {
            type: "strokes",
            strokes: [
              { type: "text", x: 10, y: 10, text: "events fold to exchanges" },
              { type: "text", x: 10, y: 40, text: "readings are stamped" },
            ],
          },
        },
        { id: "closer", kind: "title", role: "closer", line: "The next word is trust.", caption: null, stage: { type: "text" } },
      ],
    },
    scan: { v: 1, findings: 0, acknowledged: false },
    ...over,
  };
}

describe("when a bundle is baked as a Kindle edition", () => {
  const out = bakeKindleHtml(bundle());

  it("should contain no script, canvas, iframe, or external asset", () => {
    expect(out.html).not.toMatch(/<script\b/i);
    expect(out.html).not.toMatch(/<(canvas|iframe)\b/i);
    expect(out.html).not.toMatch(/(src|href)=["']https?:/i);
  });

  it("should escape the title and use it as the document title and h1", () => {
    expect(out.html).toContain("<title>The arc &lt;of&gt; OpenStory</title>");
    expect(out.html).toContain("<h1>The arc &lt;of&gt; OpenStory</h1>");
    expect(out.html).not.toContain("<of>");
  });

  it("should render the opener before the parts and the closer after them", () => {
    const opener = out.html.indexOf("Three months, one pattern.");
    const part1 = out.html.indexOf("Here is the shape of the quarter.");
    const closer = out.html.indexOf("The next word is trust.");
    expect(opener).toBeGreaterThan(-1);
    expect(opener).toBeLessThan(part1);
    expect(part1).toBeLessThan(closer);
  });

  it("should embed an image stage as a figure with the slide title as alt text", () => {
    expect(out.html).toContain(`<img src="${PNG}" alt="Commits on master, by week">`);
    expect(out.html).toContain("<figcaption>Commits on master, by week</figcaption>");
  });

  it("should render a snapshot stage as prose, not markup, with a provenance line", () => {
    expect(out.html).toContain("tell me more about openstory");
    expect(out.html).not.toContain("<div><p>tell me more");
    expect(out.html).toContain("Source: session 917baaad-5bf7 · event 73343c69-38f9");
  });

  it("should render a diagram's text strokes as an ordered list so the content survives image loss", () => {
    expect(out.html).toMatch(/<ol class="?diagram"?>\s*<li>events fold to exchanges<\/li>\s*<li>readings are stamped<\/li>\s*<\/ol>/);
  });

  it("should use each slide's title as its heading and 'Part N' when there is none", () => {
    expect(out.html).toContain("<h2>Commits on master, by week</h2>");
    expect(out.html).toContain("<h2>Part 2</h2>");
  });

  it("should carry the scan receipt as an HTML comment and a colophon line", () => {
    expect(out.html).toContain("<!-- openstory-scan findings=0 acknowledged=false -->");
    expect(out.html).toContain("Exported from OpenStory");
    expect(out.html).toContain("by Max");
  });

  it("should count the narration words", () => {
    // 4 + 7 + 5 + 2 + 5
    expect(out.wordCount).toBe(23);
  });

  it("should report no warnings for a fully renderable bundle", () => {
    expect(out.warnings).toEqual([]);
  });
});

describe("when an image stage is not a raster data URL", () => {
  it("should keep the caption, skip the image, and warn", () => {
    const b = bundle();
    const slides = b.reel.slides.map((s) =>
      s.id === "s1" ? { ...s, stage: { type: "image" as const, dataUri: "data:image/svg+xml;base64,PHN2Zz4=" } } : s,
    );
    const out = bakeKindleHtml({ ...b, reel: { ...b.reel, slides } });
    expect(out.html).not.toContain("<img");
    expect(out.html).toContain("Here is the shape of the quarter.");
    expect(out.warnings).toEqual(["Commits on master, by week: image could not be embedded; caption retained."]);
  });
});

describe("when the scan was acknowledged with findings", () => {
  it("should say so in the receipt, never 'clean'", () => {
    const out = bakeKindleHtml(bundle({ scan: { v: 1, findings: 2, acknowledged: true } }));
    expect(out.html).toContain("<!-- openstory-scan findings=2 acknowledged=true -->");
    expect(out.html).toContain("2 findings acknowledged");
    expect(out.html).not.toContain("scan: clean");
  });
});
