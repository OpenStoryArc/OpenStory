import { describe, it, expect } from "vitest";
import { renderHook } from "@testing-library/react";
import { useConnectionStatus } from "@/hooks/use-connection-status";

describe("useConnectionStatus", () => {
  it("should return 'disconnected' as initial status", () => {
    const { result } = renderHook(() => useConnectionStatus());
    expect(result.current).toBe("disconnected");
  });
});
