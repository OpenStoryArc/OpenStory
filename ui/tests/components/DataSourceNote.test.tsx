import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { DataSourceNote } from "@/components/admin/DataSourceNote";

describe("DataSourceNote", () => {
  it("shows the endpoint, kind, and how the data is derived", () => {
    render(
      <DataSourceNote
        endpoint="GET /api/admin/topology"
        derivation="self identity from config.toml"
        kind="local"
      />,
    );
    const el = screen.getByTestId("data-source");
    expect(el.textContent).toContain("GET /api/admin/topology");
    expect(el.textContent).toContain("self identity from config.toml");
    expect(el.textContent?.toLowerCase()).toContain("deterministic");
  });

  it("marks a live-network source distinctly from a local one", () => {
    render(<DataSourceNote endpoint="x" derivation="y" kind="live" />);
    expect(screen.getByTestId("data-source").textContent?.toLowerCase()).toContain(
      "live",
    );
  });

  it("marks a mixed source as part-live", () => {
    render(<DataSourceNote endpoint="x" derivation="y" kind="mixed" />);
    const t = screen.getByTestId("data-source").textContent?.toLowerCase() ?? "";
    expect(t).toContain("local");
    expect(t).toContain("probe");
  });
});
