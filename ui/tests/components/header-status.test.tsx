/** HeaderStatus — the right side of the app header: theme, text size, the
 *  jump-to button, the driven-by badge, the node's health dot, and the
 *  connection light. The health dot (H-08) has to be here, in the header
 *  the app renders, not in a component nothing mounts. */

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { HeaderStatus } from "@/components/layout/HeaderStatus";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function stubHealth(body: unknown) {
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) })),
  );
}

describe("when the header renders", () => {
  it("should place the health dot beside the connection light", async () => {
    stubHealth({ status: "ok", boot: { phase: "serving" }, bus: { connected: true }, streams: [], consumers: {}, watchers_detail: [] });
    render(<HeaderStatus status="connected" drivenBy={null} />);
    const dot = await waitFor(() => screen.getByTestId("health-dot"));
    const connection = screen.getByTestId("connection-status");
    expect(connection).toHaveTextContent("Connected");
    // The dot comes right before the connection light in the cluster.
    expect(dot.compareDocumentPosition(connection) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await waitFor(() => expect(dot.getAttribute("data-level")).toBe("ok"));
  });

  it("should show the driven-by badge only when an agent drives", () => {
    stubHealth({});
    render(<HeaderStatus status="connecting" drivenBy="mcp" />);
    expect(screen.getByTestId("driven-by")).toHaveTextContent("driven by mcp");
    expect(screen.getByTestId("connection-status")).toHaveTextContent("Connecting");
    cleanup();
    render(<HeaderStatus status="disconnected" drivenBy={null} />);
    expect(screen.queryByTestId("driven-by")).toBeNull();
    expect(screen.getByTestId("connection-status")).toHaveTextContent("Disconnected");
  });
});
