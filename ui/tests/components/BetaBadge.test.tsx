import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { BetaBadge } from "@/components/admin/BetaBadge";

describe("BetaBadge", () => {
  it("renders a visible Beta label", () => {
    render(<BetaBadge />);
    expect(screen.getByTestId("beta-badge").textContent).toContain("Beta");
  });

  it("carries a default disclaimer that the feature is unverified", () => {
    render(<BetaBadge />);
    // The tooltip/aria text must make clear this is not guaranteed to work.
    const badge = screen.getByTestId("beta-badge");
    const disclaimer = badge.getAttribute("title") ?? "";
    expect(disclaimer.toLowerCase()).toContain("not guaranteed");
    expect(disclaimer.toLowerCase()).toContain("test");
  });

  it("uses a custom note when provided", () => {
    render(<BetaBadge note="Sharing is experimental — verify before relying on it." />);
    expect(screen.getByTestId("beta-badge").getAttribute("title")).toContain(
      "experimental",
    );
  });
});
