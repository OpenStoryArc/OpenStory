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
});
