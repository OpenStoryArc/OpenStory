import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SessionVizLoader } from "@/components/viz/SessionVizLoader";

/** Minimal fetch stub: records endpoint → an array; conversation → {entries}. */
const records = [
  { record_type: "assistant_message", payload: { content: [{ type: "text", text: "hi" }] }, timestamp: "2026-07-02T00:00:00Z" },
];
const conversation = { entries: [] };

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string) =>
      Promise.resolve({
        json: () => Promise.resolve(String(url).endsWith("/conversation") ? conversation : records),
      }),
    ),
  );
});
afterEach(() => vi.unstubAllGlobals());

describe("SessionVizLoader — conversation at the forefront", () => {
  it("defaults to the Conversation view (not Tool trace)", async () => {
    render(<SessionVizLoader sessionId="s1" />);
    const convo = await screen.findByRole("button", { name: /^conversation$/i });
    expect(convo.getAttribute("aria-pressed")).toBe("true");
    const trace = screen.getByRole("button", { name: /^tool trace$/i });
    expect(trace.getAttribute("aria-pressed")).toBe("false");
  });

  it("keeps the Tool trace one click away", async () => {
    render(<SessionVizLoader sessionId="s1" />);
    const trace = await screen.findByRole("button", { name: /^tool trace$/i });
    fireEvent.click(trace);
    await waitFor(() => expect(trace.getAttribute("aria-pressed")).toBe("true"));
    expect(screen.getByRole("button", { name: /^conversation$/i }).getAttribute("aria-pressed")).toBe("false");
  });
});
