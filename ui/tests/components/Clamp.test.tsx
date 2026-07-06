import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Clamp } from "@/components/ui/Clamp";

const LONG = "This is a long piece of text that would otherwise be hard-truncated with no way to read the rest of it.";

describe("Clamp — clipped text that is always reachable", () => {
  it("renders the FULL text in the DOM (never drops it)", () => {
    render(<Clamp text={LONG} />);
    expect(screen.getByTestId("clamp").textContent).toBe(LONG);
  });

  it("exposes the full text as a title tooltip while collapsed (reachable on hover)", () => {
    render(<Clamp text={LONG} />);
    expect(screen.getByTestId("clamp").getAttribute("title")).toBe(LONG);
  });

  it("expands on click — data-open flips and the clamp title is cleared", () => {
    render(<Clamp text={LONG} />);
    const el = screen.getByTestId("clamp");
    expect(el.getAttribute("data-open")).toBe("false");
    fireEvent.click(el);
    expect(el.getAttribute("data-open")).toBe("true");
    expect(el.getAttribute("title")).toBeNull();
  });

  it("toggles back to collapsed on a second click", () => {
    render(<Clamp text={LONG} />);
    const el = screen.getByTestId("clamp");
    fireEvent.click(el);
    fireEvent.click(el);
    expect(el.getAttribute("data-open")).toBe("false");
  });

  it("is keyboard-operable (Enter expands)", () => {
    render(<Clamp text={LONG} />);
    const el = screen.getByTestId("clamp");
    fireEvent.keyDown(el, { key: "Enter" });
    expect(el.getAttribute("data-open")).toBe("true");
  });

  it("renders empty-safe for missing text", () => {
    render(<Clamp text={""} />);
    expect(screen.getByTestId("clamp").textContent).toBe("");
  });
});
