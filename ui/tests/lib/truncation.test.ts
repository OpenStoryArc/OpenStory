//! Spec: Story 045 — Truncation badge wording and helpers.
//!
//! The truncation badge should show how much was hidden, not the total size.
//! Copy-to-clipboard and "view full" are UI affordances tested here as pure logic.

import { describe, it, expect, vi } from "vitest";
import { formatBytes, truncationLabel } from "@/lib/truncation";

describe("formatBytes", () => {
  const cases: [number, string][] = [
    [0, "0 B"],
    [100, "100 B"],
    [1023, "1023 B"],
    [1024, "1.0 KB"],
    [2048, "2.0 KB"],
    [10240, "10.0 KB"],
    [1048576, "1.0 MB"],
    [5242880, "5.0 MB"],
  ];

  it.each(cases)("%d bytes → %s", (bytes, expected) => {
    expect(formatBytes(bytes)).toBe(expected);
  });
});

describe("truncationLabel", () => {
  it("shows amount hidden when payload is larger than display limit", () => {
    // payload_bytes=10240 (10KB), displayed=2000 chars (~2KB)
    const label = truncationLabel(10240, 2000);
    expect(label).toContain("8.0 KB");
    expect(label).toContain("hidden");
  });

  it("shows 'showing X of Y' format", () => {
    const label = truncationLabel(5120, 2000);
    expect(label).toMatch(/showing.*of/i);
  });

  it("handles case where payload_bytes is close to display limit", () => {
    const label = truncationLabel(2100, 2000);
    expect(label).toContain("hidden");
  });

  it("handles zero displayed chars gracefully", () => {
    const label = truncationLabel(5000, 0);
    expect(label).toBeDefined();
  });
});

describe("contentApiUrl", () => {
  // Imported inline to keep test self-contained
  it("builds correct URL from session_id and event_id", async () => {
    const { contentApiUrl } = await import("@/lib/truncation");
    const url = contentApiUrl("sess-123", "evt-456");
    expect(url).toBe("/api/sessions/sess-123/events/evt-456/content");
  });
});

describe("copyToClipboard", () => {
  it("returns true when clipboard write succeeds", async () => {
    const { copyToClipboard } = await import("@/lib/truncation");
    // Mock navigator.clipboard
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
    const result = await copyToClipboard("test text");
    expect(result).toBe(true);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("test text");
  });

  it("returns false when clipboard write fails", async () => {
    const { copyToClipboard } = await import("@/lib/truncation");
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    const result = await copyToClipboard("test text");
    expect(result).toBe(false);
  });
});
