/** The mirror must come back on its own. A WebSocket stream dies two ways —
 *  an ERROR (network drop) or a clean COMPLETE (server restart closes the
 *  socket) — and the connection policy must resubscribe after BOTH.
 *  (The old chain had catchError→EMPTY before retry: errors became completes,
 *  retry never fired, and a server restart left every open dashboard dead
 *  until a manual reload.) */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { Observable } from "rxjs";
import { resilient } from "@/streams/connection";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

function source(behaviors: ("error" | "complete" | "live")[]) {
  let attempt = 0;
  const factory = vi.fn(
    () =>
      new Observable<string>((sub) => {
        const b = behaviors[attempt++];
        if (b === "error") sub.error(new Error("net down"));
        else if (b === "complete") sub.complete();
        else sub.next(`msg-${attempt}`);
      }),
  );
  return factory;
}

describe("when the socket ERRORS (network drop)", () => {
  it("should resubscribe after the delay and keep delivering", () => {
    const factory = source(["error", "error", "live"]);
    const got: string[] = [];
    const sub = resilient(factory, 2000).subscribe((m) => got.push(m));

    expect(factory).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(2000);
    expect(factory).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(2000);
    expect(factory).toHaveBeenCalledTimes(3);
    expect(got).toEqual(["msg-3"]);
    sub.unsubscribe();
  });
});

describe("when the socket COMPLETES cleanly (server restart)", () => {
  it("should resubscribe after the delay — a restart must not strand the page", () => {
    const factory = source(["complete", "live"]);
    const got: string[] = [];
    const sub = resilient(factory, 2000).subscribe((m) => got.push(m));

    expect(factory).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(2000);
    expect(factory).toHaveBeenCalledTimes(2);
    expect(got).toEqual(["msg-2"]);
    sub.unsubscribe();
  });
});

describe("when unsubscribed", () => {
  it("should stop resubscribing (no zombie reconnect loops)", () => {
    const factory = source(["error", "error", "error"]);
    const sub = resilient(factory, 2000).subscribe();
    sub.unsubscribe();
    vi.advanceTimersByTime(10000);
    expect(factory).toHaveBeenCalledTimes(1);
  });
});
