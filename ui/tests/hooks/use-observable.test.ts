import { describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { BehaviorSubject, Subject } from "rxjs";
import { useObservable } from "@/hooks/use-observable";

describe("useObservable", () => {
  it("should return the initial value before first emission", () => {
    const subject = new Subject<number>();
    const { result } = renderHook(() => useObservable(subject, 42));
    expect(result.current).toBe(42);
  });

  it("should update when the observable emits a new value", () => {
    const subject = new BehaviorSubject<string>("hello");
    const { result } = renderHook(() => useObservable(subject, "initial"));

    expect(result.current).toBe("hello");

    act(() => subject.next("world"));
    expect(result.current).toBe("world");
  });

  it("should unsubscribe when the component unmounts", () => {
    const subject = new BehaviorSubject<number>(1);
    const { unmount } = renderHook(() => useObservable(subject, 0));

    expect(subject.observed).toBe(true);
    unmount();
    expect(subject.observed).toBe(false);
  });

  it("should resubscribe when the observable reference changes", () => {
    const subject1 = new BehaviorSubject<string>("first");
    const subject2 = new BehaviorSubject<string>("second");

    const { result, rerender } = renderHook(
      ({ obs$ }) => useObservable(obs$, "initial"),
      { initialProps: { obs$: subject1 as any } },
    );

    expect(result.current).toBe("first");

    rerender({ obs$: subject2 as any });
    expect(result.current).toBe("second");

    // Old subject should be unsubscribed
    expect(subject1.observed).toBe(false);
    expect(subject2.observed).toBe(true);
  });

  it("should handle synchronous emissions on subscribe", () => {
    const subject = new BehaviorSubject<number>(99);
    const { result } = renderHook(() => useObservable(subject, 0));

    // BehaviorSubject emits synchronously on subscribe
    expect(result.current).toBe(99);
  });
});
