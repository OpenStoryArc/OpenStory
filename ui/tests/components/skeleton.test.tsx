import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { Skeleton } from "@/components/ui/skeleton";

describe("Skeleton", () => {
  it("renders a shimmering placeholder", () => {
    const { container } = render(<Skeleton />);
    const el = container.querySelector('[data-slot="skeleton"]');
    expect(el).toBeInTheDocument();
    expect(el).toHaveClass("skeleton-shimmer");
  });

  it("merges a caller className via cn (tailwind-merge)", () => {
    const { container } = render(<Skeleton className="h-4 w-1/2" />);
    const el = container.querySelector('[data-slot="skeleton"]')!;
    expect(el).toHaveClass("h-4");
    expect(el).toHaveClass("w-1/2");
    // base classes still present
    expect(el).toHaveClass("skeleton-shimmer");
  });
});
