import { describe, it, expect } from "vitest";
import { fullTimestamp } from "@/lib/time";

/** The date matters — every session/event should show a clean, absolute
 *  "HH:MM:SS TZ YYYY/MM/DD" so you always know WHEN, not just a bare time. */
describe("fullTimestamp — clean absolute HH:MM:SS TZ YYYY/MM/DD", () => {
  it("formats a known ISO in a fixed timezone deterministically", () => {
    expect(fullTimestamp("2026-07-02T14:27:21Z", { timeZone: "UTC" })).toBe("14:27:21 UTC 2026/07/02");
  });

  it("respects the given timezone (offset shifts the clock + date)", () => {
    // 00:30 UTC on the 2nd is 20:30 on the 1st in New York (-4). The tz token's
    // exact spelling (EDT vs GMT-4) depends on the runtime's ICU data, so assert
    // the shifted clock + date and that SOME tz token is present.
    const s = fullTimestamp("2026-07-02T00:30:00Z", { timeZone: "America/New_York" });
    expect(s).toMatch(/^20:30:00 \S+ 2026\/07\/01$/);
  });

  it("uses 24-hour time", () => {
    expect(fullTimestamp("2026-07-02T23:05:09Z", { timeZone: "UTC" })).toBe("23:05:09 UTC 2026/07/02");
  });

  it("always shapes as time TZ then date", () => {
    expect(fullTimestamp("2026-01-09T03:04:05Z", { timeZone: "UTC" })).toMatch(/^\d{2}:\d{2}:\d{2} \S+ \d{4}\/\d{2}\/\d{2}$/);
  });

  it("returns an em-dash for invalid or empty input (never 'Invalid Date')", () => {
    expect(fullTimestamp("")).toBe("—");
    expect(fullTimestamp("not-a-date")).toBe("—");
  });
});
