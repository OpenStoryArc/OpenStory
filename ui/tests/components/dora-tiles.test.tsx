/** DoraTiles — the four keys on the Admin tab from GET /api/dora, with the
 *  window selectable (D-06). */

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DoraTiles } from "@/components/admin/DoraTiles";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const json = {
  generated_at: "2026-09-24T01:00:00Z",
  windows: {
    "7": { window_days: 7, deployments: 3, deployment_frequency_per_day: 0.429, lead_time_hours_median: 2.5, change_failure_rate: 0.333, time_to_restore_minutes_median: 15, open_criticals: 1 },
    "30": { window_days: 30, deployments: 9, deployment_frequency_per_day: 0.3, lead_time_hours_median: 6, change_failure_rate: 0.111, time_to_restore_minutes_median: 42, open_criticals: 0 },
  },
};

function stub(status: number, body: unknown) {
  vi.stubGlobal("fetch", vi.fn(() => Promise.resolve({ ok: status < 400, status, json: () => Promise.resolve(body) })));
}

describe("when dora json loads", () => {
  it("should render four keys", async () => {
    stub(200, json);
    render(<DoraTiles />);
    const freq = await waitFor(() => screen.getByTestId("dora-tile-deployment_frequency"));
    expect(freq).toHaveTextContent("0.43");
    expect(screen.getByTestId("dora-tile-lead_time")).toHaveTextContent("2.5");
    expect(screen.getByTestId("dora-tile-change_failure_rate")).toHaveTextContent("33");
    expect(screen.getByTestId("dora-tile-time_to_restore")).toHaveTextContent("15");
    expect(screen.getByTestId("dora-tiles")).toHaveTextContent("2026-09-24");

    const select = screen.getByTestId("dora-window") as HTMLSelectElement;
    expect(Array.from(select.options).map((o) => o.value)).toEqual(["7", "30"]);
    fireEvent.change(select, { target: { value: "30" } });
    await waitFor(() => expect(screen.getByTestId("dora-tile-lead_time")).toHaveTextContent("6"));
    expect(screen.getByTestId("dora-tile-change_failure_rate")).toHaveTextContent("11");
  });
});

describe("when no dora json has been written", () => {
  it("should say how to make one", async () => {
    stub(404, { error: "no dora.json in the data dir; run scripts/dora.py --write" });
    render(<DoraTiles />);
    await waitFor(() => expect(screen.getByTestId("dora-tiles")).toHaveTextContent("scripts/dora.py --write"));
  });
});
