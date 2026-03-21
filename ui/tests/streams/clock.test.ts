import { describe, it, expect, vi } from "vitest";
import { firstValueFrom } from "rxjs";
import { take, toArray } from "rxjs/operators";

describe("tick$", () => {
  it("should emit the current timestamp on subscribe", async () => {
    // Import fresh to avoid shared state
    const { tick$ } = await import("@/streams/clock");
    const value = await firstValueFrom(tick$);
    expect(typeof value).toBe("number");
    expect(Math.abs(value - Date.now())).toBeLessThan(1000);
  });

  it("should emit multiple values over time", async () => {
    vi.useFakeTimers();
    // Re-import to get fresh interval with fake timers
    vi.resetModules();
    const { tick$ } = await import("@/streams/clock");

    const promise = firstValueFrom(tick$.pipe(take(3), toArray()));
    vi.advanceTimersByTime(5000);
    const values = await promise;

    expect(values).toHaveLength(3);
    vi.useRealTimers();
  });
});
