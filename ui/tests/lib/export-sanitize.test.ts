import { describe, expect, it } from "vitest";
import { sanitizeSnapshotHtml } from "@/lib/export-sanitize";

describe("sanitizeSnapshotHtml", () => {
  it("strips script and iframe elements entirely", () => {
    const out = sanitizeSnapshotHtml(
      "<div>ok<script>alert(1)</script><iframe src=\"x\"></iframe></div>",
    );
    expect(out).toContain("ok");
    expect(out).not.toContain("script");
    expect(out).not.toContain("iframe");
  });

  it("strips event handlers and javascript: URLs", () => {
    const out = sanitizeSnapshotHtml(
      "<a href=\"javascript:evil()\" onclick=\"evil()\">x</a>",
    );
    expect(out).not.toContain("onclick");
    expect(out).not.toContain("javascript:");
  });

  it("drops external images but keeps data: images", () => {
    const out = sanitizeSnapshotHtml(
      "<img src=\"https://evil.example/x.png\"><img src=\"data:image/png;base64,AA==\">",
    );
    expect(out).not.toContain("evil.example");
    expect(out).toContain("data:image/png");
  });

  it("keeps class names, inline styles, and text", () => {
    const out = sanitizeSnapshotHtml(
      "<div class=\"card\" style=\"color:red\">payload</div>",
    );
    expect(out).toContain("class=\"card\"");
    expect(out).toContain("color:red");
    expect(out).toContain("payload");
  });

  it("strips url() references to external hosts inside style attributes", () => {
    const out = sanitizeSnapshotHtml(
      "<div style=\"background:url(https://evil.example/x)\">x</div>",
    );
    expect(out).not.toContain("evil.example");
  });

  it("is idempotent", () => {
    const once = sanitizeSnapshotHtml("<div class=\"a\" onclick=\"e()\">t</div>");
    expect(sanitizeSnapshotHtml(once)).toBe(once);
  });

  it("strips script elements inside SVG (namespace-aware)", () => {
    const out = sanitizeSnapshotHtml(
      "<svg><script>alert(1)</script><circle cx=\"50\" cy=\"50\" r=\"40\"/></svg>",
    );
    expect(out).not.toContain("<script>");
    expect(out).not.toContain("alert");
    expect(out).toContain("circle");
  });

  it("strips style elements inside SVG with @import exfil attempts", () => {
    const out = sanitizeSnapshotHtml(
      "<svg><style>@import url(https://evil.example/x.css)</style></svg>",
    );
    expect(out).not.toContain("<style>");
    expect(out).not.toContain("@import");
    expect(out).not.toContain("evil.example");
  });

  it("keeps benign SVG shapes (svg, circle, path, rect, g)", () => {
    const out = sanitizeSnapshotHtml(
      "<svg><g><circle cx=\"50\" cy=\"50\" r=\"40\"/><path d=\"M 0 0 L 10 10\"/><rect x=\"0\" y=\"0\" width=\"10\" height=\"10\"/></g></svg>",
    );
    expect(out).toContain("<svg");
    expect(out).toContain("circle");
    expect(out).toContain("path");
    expect(out).toContain("rect");
    expect(out).toContain("<g");
  });

  it("is idempotent on SVG with script/style stripped", () => {
    const once = sanitizeSnapshotHtml(
      "<svg><script>x</script><circle/></svg>",
    );
    expect(sanitizeSnapshotHtml(once)).toBe(once);
    expect(once).not.toContain("<script>");
  });

  it("strips external url() funcIRIs from fill/stroke attributes", () => {
    const out = sanitizeSnapshotHtml(
      "<svg><rect fill=\"url(https://evil.example/exfil.svg#leak)\" stroke=\"url(https://evil.example/stroke)\"/></svg>",
    );
    expect(out).not.toContain("evil.example");
    expect(out).not.toContain("exfil.svg");
  });

  it("keeps same-document funcIRI refs and benign fill/stroke values", () => {
    const out = sanitizeSnapshotHtml(
      "<svg><rect fill=\"url(#gradient)\" stroke=\"#ff0000\" stroke-width=\"2\"/><defs><linearGradient id=\"gradient\"><stop/></linearGradient></defs></svg>",
    );
    expect(out).toContain("url(#gradient)");
    expect(out).toContain("#ff0000");
    expect(out).toContain("stroke-width");
  });
});
