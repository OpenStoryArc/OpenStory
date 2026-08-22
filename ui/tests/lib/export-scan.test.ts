import { describe, expect, it } from "vitest";
import { scanBundle } from "@/lib/export-scan";
import { buildBundle } from "@/lib/reel-bundle";
import type { Reel } from "@/lib/reels-api";

function reelWithLine(line: string): Reel {
  return { id: "r", title: "t", author: "a", created: "c", stops: [{ line, kind: "title" }] } as Reel;
}
const scanOf = (line: string) =>
  scanBundle(buildBundle(reelWithLine(line), new Map(), new Map(), { exportedBy: "x" }));

describe("scanBundle families", () => {
  it("flags AWS access key ids", () => {
    expect(scanOf("key AKIAIOSFODNN7EXAMPLE here")[0]?.family).toBe("aws-key");
  });
  it("flags secret-assignment shapes", () => {
    expect(scanOf("api_key = \"sk-live-abcdef1234567890\"").length).toBeGreaterThan(0);
  });
  it("flags PEM blocks", () => {
    expect(scanOf("-----BEGIN RSA PRIVATE KEY-----")[0]?.family).toBe("pem");
  });
  it("flags bearer tokens", () => {
    expect(scanOf("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.x.y").length).toBeGreaterThan(0);
  });
  it("flags email addresses", () => {
    expect(scanOf("mail me at someone@example.com")[0]?.family).toBe("email");
  });
  it("flags absolute home paths", () => {
    expect(scanOf("read /Users/somebody/secrets.txt")[0]?.family).toBe("home-path");
  });
  it("stays quiet on ordinary prose and trims excerpts to 80 chars", () => {
    expect(scanOf("We fixed the caption bar and merged the PR.")).toEqual([]);
    const f = scanOf("x".repeat(200) + " AKIAIOSFODNN7EXAMPLE");
    expect(f[0]!.excerpt.length).toBeLessThanOrEqual(80);
  });
});
