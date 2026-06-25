import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { HowItWorks } from "@/components/admin/HowItWorks";

describe("HowItWorks", () => {
  it("renders the summary and its detail content", () => {
    const { container } = render(
      <HowItWorks summary="How share policy works under the hood">
        <p>three independent gates</p>
      </HowItWorks>,
    );
    const text = container.textContent ?? "";
    expect(text).toContain("How share policy works under the hood");
    expect(text).toContain("three independent gates");
  });
});
