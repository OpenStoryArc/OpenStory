import { describe, it, expect, vi, afterEach } from "vitest";
import { scenario } from "../bdd";
import {
  formatDuration,
  relativeTime,
  relativeTimeFrom,
  compactTime,
  compactTimeFrom,
  formatTime,
} from "@/lib/time";

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

describe("compactTimeFrom — Eastern date + time + zone", () => {
  // 2026-06-09T01:53:59Z == 2026-06-08 21:53:59 in America/New_York (EDT, UTC-4)
  const NOW_EDT = new Date("2026-06-09T01:53:59Z").getTime();

  it("should label same Eastern day as Today, with EDT in summer", () => {
    scenario(
      () => "2026-06-09T01:53:59Z",
      (iso) => compactTimeFrom(iso, NOW_EDT),
      (result) => expect(result).toBe("Today 21:53:59 EDT"),
    );
  });

  it("should label the previous Eastern day as Yesterday", () => {
    scenario(
      () => "2026-06-08T20:00:00Z", // Jun 8 16:00 EDT, now is Jun 8 21:53 EDT → same day? no: now=Jun8, iso=Jun8
      (iso) => compactTimeFrom(iso, new Date("2026-06-09T20:00:00Z").getTime()),
      (result) => expect(result).toBe("Yesterday 16:00:00 EDT"),
    );
  });

  it("should show 'Mon DD' (no year) + EST for an older date in the current year", () => {
    scenario(
      () => "2026-01-15T17:44:49Z", // Jan 15 12:44:49 EST
      (iso) => compactTimeFrom(iso, NOW_EDT),
      (result) => expect(result).toBe("Jan 15 12:44:49 EST"),
    );
  });

  it("should include the year for a date in a prior year", () => {
    scenario(
      () => "2025-12-03T14:12:00Z", // Dec 3 2025 09:12:00 EST
      (iso) => compactTimeFrom(iso, NOW_EDT),
      (result) => expect(result).toBe("Dec 3 2025 09:12:00 EST"),
    );
  });

  it("compactTime delegates to compactTimeFrom(now)", () => {
    scenario(
      () => {
        vi.spyOn(Date, "now").mockReturnValue(NOW_EDT);
        return "2026-06-09T01:53:59Z";
      },
      (iso) => compactTime(iso),
      (result) => {
        expect(result).toBe("Today 21:53:59 EDT");
        vi.restoreAllMocks();
      },
    );
  });

  it("returns empty string for an invalid timestamp", () => {
    scenario(
      () => "not-a-date",
      (iso) => compactTimeFrom(iso, NOW_EDT),
      (result) => expect(result).toBe(""),
    );
  });
});

describe("formatTime — absolute Eastern timestamp", () => {
  it("always shows the date (no Today/Yesterday), EST in winter", () => {
    scenario(
      () => "2026-01-15T14:30:00Z", // Jan 15 09:30:00 EST
      (iso) => formatTime(iso),
      (result) => expect(result).toBe("Jan 15 2026 09:30:00 EST"),
    );
  });

  it("uses EDT in summer", () => {
    scenario(
      () => "2026-06-09T01:53:59Z", // Jun 8 21:53:59 EDT
      (iso) => formatTime(iso),
      (result) => expect(result).toBe("Jun 8 2026 21:53:59 EDT"),
    );
  });
});
