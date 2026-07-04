/** Storm polish: a sticky is a SHAREABLE place (#/storm?sticky=id opens
 *  with it selected; selecting writes the hash), and hovering any sticky
 *  teaches without clicking (label + note in the title). */

import { describe, it, expect, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { EventStormBoard } from "@/components/storm/EventStormBoard";

afterEach(() => {
  window.history.replaceState(null, "", "#/storm");
});

describe("when the URL deep-links a sticky", () => {
  it("should open with that sticky selected (its note in the detail panel)", () => {
    window.history.replaceState(null, "", "#/storm?sticky=rm_recordcache");
    render(<EventStormBoard />);
    // The detail panel shows the record-cache sticky's note.
    expect(screen.getByText(/fetched ONCE, shared by every surface/)).toBeInTheDocument();
  });
});

describe("when a sticky is selected by click", () => {
  it("should write the shareable hash", () => {
    window.history.replaceState(null, "", "#/storm");
    render(<EventStormBoard />);
    fireEvent.click(screen.getByTestId("sticky-rm_recordcache"));
    expect(window.location.hash).toBe("#/storm?sticky=rm_recordcache");
  });
});

describe("when hovering a sticky", () => {
  it("should teach with label + note in the title", () => {
    render(<EventStormBoard />);
    const title = screen.getByTestId("sticky-rm_recordcache").getAttribute("title");
    expect(title).toContain("Record cache");
    expect(title).toContain("fetched ONCE");
  });
});
