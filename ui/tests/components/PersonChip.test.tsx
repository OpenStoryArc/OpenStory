/** Spec: PersonChip — primary user pill in the Live tab's PersonRow. */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { PersonChip } from "@/components/PersonChip";

function setup(overrides: Partial<Parameters<typeof PersonChip>[0]> = {}) {
  const onClick = vi.fn();
  const props = {
    user: "katie",
    sessionCount: 32,
    selected: false,
    isActiveNow: false,
    onClick,
    ...overrides,
  };
  render(<PersonChip {...props} />);
  return { onClick };
}

describe("PersonChip", () => {
  it("renders the @user name + session count", () => {
    setup();
    expect(screen.getByText("@katie")).toBeTruthy();
    expect(screen.getByText("32 sessions")).toBeTruthy();
  });

  it("singularizes 'session' for count = 1", () => {
    setup({ sessionCount: 1 });
    expect(screen.getByText("1 session")).toBeTruthy();
  });

  it("calls onClick when clicked", () => {
    const { onClick } = setup();
    fireEvent.click(screen.getByTestId("person-chip-katie"));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("exposes selected state via data-selected for styling assertions", () => {
    setup({ selected: true });
    const chip = screen.getByTestId("person-chip-katie");
    expect(chip.getAttribute("data-selected")).toBe("true");
  });

  it("renders the active-now pulse dot when isActiveNow is true", () => {
    setup({ isActiveNow: true });
    expect(screen.getByLabelText("active now")).toBeTruthy();
  });

  it("omits the active-now pulse dot when isActiveNow is false", () => {
    setup({ isActiveNow: false });
    expect(screen.queryByLabelText("active now")).toBeNull();
  });

  it("renders an avatar with the first two characters uppercased", () => {
    setup({ user: "maxglassie" });
    expect(screen.getByText("MA")).toBeTruthy();
  });

  it("title prompts to clear when selected, to filter when not", () => {
    const { rerender } = render(
      <PersonChip user="katie" sessionCount={1} selected={false} isActiveNow={false} onClick={() => {}} />,
    );
    expect(
      screen.getByTestId("person-chip-katie").getAttribute("title"),
    ).toMatch(/Filter to katie/);

    rerender(
      <PersonChip user="katie" sessionCount={1} selected={true} isActiveNow={false} onClick={() => {}} />,
    );
    expect(
      screen.getByTestId("person-chip-katie").getAttribute("title"),
    ).toMatch(/Clear katie filter/);
  });
});
