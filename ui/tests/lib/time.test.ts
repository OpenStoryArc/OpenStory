import { describe, it, expect, vi, afterEach } from "vitest";
import { scenario } from "../bdd";
import { formatDuration, relativeTime, relativeTimeFrom, compactTime } from "@/lib/time";

describe("formatDuration", () => {
  it("should format sub-second durations as milliseconds", () => {
    scenario(
      () => 500,
      (ms) => formatDuration(ms),
      (result) => expect(result).toBe("500ms"),
    );
  });

  it("should format zero as 0ms", () => {
    scenario(
      () => 0,
      (ms) => formatDuration(ms),
      (result) => expect(result).toBe("0ms"),
    );
  });

  it("should format exact seconds without minutes", () => {
    scenario(
      () => 5000,
      (ms) => formatDuration(ms),
      (result) => expect(result).toBe("5s"),
    );
  });

  it("should format seconds at the boundary (999ms)", () => {
    scenario(
      () => 999,
      (ms) => formatDuration(ms),
      (result) => expect(result).toBe("999ms"),
    );
  });

  it("should format minutes with remaining seconds", () => {
    scenario(
      () => 65000,
      (ms) => formatDuration(ms),
      (result) => expect(result).toBe("1m 5s"),
    );
  });

  it("should format exact minutes with 0 remaining seconds", () => {
    scenario(
      () => 120000,
      (ms) => formatDuration(ms),
      (result) => expect(result).toBe("2m 0s"),
    );
  });

  it("should format hours with remaining minutes", () => {
    scenario(
      () => 3660000,
      (ms) => formatDuration(ms),
      (result) => expect(result).toBe("1h 1m"),
    );
  });

  it("should format exact hours with 0 remaining minutes", () => {
    scenario(
      () => 7200000,
      (ms) => formatDuration(ms),
      (result) => expect(result).toBe("2h 0m"),
    );
  });
});

describe("relativeTime", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("should format times in the future as 'just now'", () => {
    scenario(
      () => {
        vi.spyOn(Date, "now").mockReturnValue(1000);
        return new Date(5000).toISOString();
      },
      (iso) => relativeTime(iso),
      (result) => expect(result).toBe("just now"),
    );
  });

  it("should format seconds ago", () => {
    scenario(
      () => {
        const now = Date.now();
        vi.spyOn(Date, "now").mockReturnValue(now);
        return new Date(now - 30000).toISOString();
      },
      (iso) => relativeTime(iso),
      (result) => expect(result).toBe("30s ago"),
    );
  });

  it("should format minutes ago", () => {
    scenario(
      () => {
        const now = Date.now();
        vi.spyOn(Date, "now").mockReturnValue(now);
        return new Date(now - 300000).toISOString();
      },
      (iso) => relativeTime(iso),
      (result) => expect(result).toBe("5m ago"),
    );
  });

  it("should format hours ago", () => {
    scenario(
      () => {
        const now = Date.now();
        vi.spyOn(Date, "now").mockReturnValue(now);
        return new Date(now - 7200000).toISOString();
      },
      (iso) => relativeTime(iso),
      (result) => expect(result).toBe("2h ago"),
    );
  });

  it("should format days ago", () => {
    scenario(
      () => {
        const now = Date.now();
        vi.spyOn(Date, "now").mockReturnValue(now);
        return new Date(now - 172800000).toISOString();
      },
      (iso) => relativeTime(iso),
      (result) => expect(result).toBe("2d ago"),
    );
  });
});

describe("relativeTimeFrom — boundary table", () => {
  const BASE = new Date("2025-01-08T12:00:00Z").getTime();

  it.each([
    // future → just now
    [BASE + 5000, "just now"],
    // seconds
    [BASE - 30_000, "30s ago"],
    // boundary: 59s
    [BASE - 59_000, "59s ago"],
    // minutes
    [BASE - 300_000, "5m ago"],
    // boundary: 59m
    [BASE - 3_540_000, "59m ago"],
    // hours
    [BASE - 7_200_000, "2h ago"],
    // boundary: 23h
    [BASE - 82_800_000, "23h ago"],
    // days
    [BASE - 172_800_000, "2d ago"],
  ])("relativeTimeFrom(%d) → %s", (isoTime, expected) => {
    const iso = new Date(isoTime).toISOString();
    expect(relativeTimeFrom(iso, BASE)).toBe(expected);
  });
});

describe("compactTime", () => {
  it("should format ISO timestamp as HH:MM:SS", () => {
    scenario(
      () => "2026-01-01T14:30:45Z",
      (iso) => compactTime(iso),
      (result) => expect(result).toMatch(/^\d{2}:\d{2}:\d{2}$/),
    );
  });
});
