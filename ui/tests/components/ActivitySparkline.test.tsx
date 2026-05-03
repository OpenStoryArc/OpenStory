/** Spec: ActivitySparkline — renders 24 bars with correct heights/colors. */

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ActivitySparkline } from "@/components/users/ActivitySparkline";

describe("ActivitySparkline", () => {
  it("renders an SVG with 24 bars", () => {
    render(<ActivitySparkline buckets={Array(24).fill(1)} color="#7aa2f7" />);
    const bars = screen
      .getByTestId("activity-sparkline")
      .querySelectorAll("rect");
    expect(bars.length).toBe(24);
  });

  it("each bar's data-bucket index goes 0..23", () => {
    render(<ActivitySparkline buckets={Array(24).fill(1)} color="#7aa2f7" />);
    const bars = screen
      .getByTestId("activity-sparkline")
      .querySelectorAll("rect");
    for (let i = 0; i < 24; i++) {
      expect(bars[i]!.getAttribute("data-bucket")).toBe(String(i));
    }
  });

  it("data-value mirrors the input bucket value", () => {
    const buckets = [0, 5, 10, 0, 3, ...Array(19).fill(0)];
    render(<ActivitySparkline buckets={buckets} color="#7aa2f7" />);
    const bars = screen
      .getByTestId("activity-sparkline")
      .querySelectorAll("rect");
    expect(bars[0]!.getAttribute("data-value")).toBe("0");
    expect(bars[1]!.getAttribute("data-value")).toBe("5");
    expect(bars[2]!.getAttribute("data-value")).toBe("10");
    expect(bars[3]!.getAttribute("data-value")).toBe("0");
  });

  it("uses the supplied color for non-zero bars", () => {
    const buckets = [0, 5, ...Array(22).fill(0)];
    render(<ActivitySparkline buckets={buckets} color="#bb9af7" />);
    const bars = screen
      .getByTestId("activity-sparkline")
      .querySelectorAll("rect");
    expect(bars[1]!.getAttribute("fill")).toBe("#bb9af7");
  });

  it("renders 24 bars even when the input array is shorter", () => {
    // Shorter array → component fills with 0s. Defends layout against
    // an upstream contract change.
    render(<ActivitySparkline buckets={[1, 2, 3]} color="#7aa2f7" />);
    const bars = screen
      .getByTestId("activity-sparkline")
      .querySelectorAll("rect");
    expect(bars.length).toBe(24);
    expect(bars[0]!.getAttribute("data-value")).toBe("1");
    expect(bars[3]!.getAttribute("data-value")).toBe("0");
    expect(bars[23]!.getAttribute("data-value")).toBe("0");
  });

  it("includes a baseline + midpoint divider for time orientation", () => {
    render(<ActivitySparkline buckets={Array(24).fill(0)} color="#7aa2f7" />);
    const lines = screen
      .getByTestId("activity-sparkline")
      .querySelectorAll("line");
    // 1 baseline + 1 midpoint = 2.
    expect(lines.length).toBe(2);
  });

  it("exposes an aria-label for screen readers", () => {
    render(
      <ActivitySparkline
        buckets={Array(24).fill(0)}
        color="#7aa2f7"
        ariaLabel="katie's activity over the last 24 hours"
      />,
    );
    expect(
      screen.getByLabelText("katie's activity over the last 24 hours"),
    ).toBeTruthy();
  });
});
