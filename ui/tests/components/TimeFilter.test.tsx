/** Spec: TimeFilter — pill-row component on the Live sidebar. */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TimeFilter } from "@/components/TimeFilter";

describe("TimeFilter", () => {
  it("renders all four pills", () => {
    render(<TimeFilter value="all" onChange={() => {}} />);
    expect(screen.getByTestId("time-filter-1h")).toBeTruthy();
    expect(screen.getByTestId("time-filter-today")).toBeTruthy();
    expect(screen.getByTestId("time-filter-week")).toBeTruthy();
    expect(screen.getByTestId("time-filter-all")).toBeTruthy();
  });

  it("displays the human-friendly labels", () => {
    render(<TimeFilter value="all" onChange={() => {}} />);
    expect(screen.getByText("Last Hour")).toBeTruthy();
    expect(screen.getByText("Today")).toBeTruthy();
    expect(screen.getByText("This Week")).toBeTruthy();
    expect(screen.getByText("All")).toBeTruthy();
  });

  it("marks the active pill with data-selected=true", () => {
    render(<TimeFilter value="today" onChange={() => {}} />);
    expect(
      screen.getByTestId("time-filter-today").getAttribute("data-selected"),
    ).toBe("true");
    expect(
      screen.getByTestId("time-filter-1h").getAttribute("data-selected"),
    ).toBe("false");
  });

  it("treats null as 'all' (no pill highlighted forces a default)", () => {
    render(<TimeFilter value={null} onChange={() => {}} />);
    expect(
      screen.getByTestId("time-filter-all").getAttribute("data-selected"),
    ).toBe("true");
  });

  it("calls onChange with the clicked key", () => {
    const onChange = vi.fn();
    render(<TimeFilter value="all" onChange={onChange} />);
    fireEvent.click(screen.getByTestId("time-filter-1h"));
    expect(onChange).toHaveBeenCalledWith("1h");
  });

  it("calls onChange even when the active pill is re-clicked (idempotent)", () => {
    const onChange = vi.fn();
    render(<TimeFilter value="today" onChange={onChange} />);
    fireEvent.click(screen.getByTestId("time-filter-today"));
    expect(onChange).toHaveBeenCalledWith("today");
  });
});
