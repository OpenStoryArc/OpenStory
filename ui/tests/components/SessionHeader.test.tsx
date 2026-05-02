/**
 * Spec: SessionHeader — context strip above the Live timeline.
 *
 * The cross-user band is the load-bearing behavior — it answers
 * "whose session is this?" without making the user click into Explore.
 */

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { SessionHeader } from "@/components/SessionHeader";

const baseSession = {
  session_id: "abcdef12-3456-7890-abcd-ef1234567890",
  user: "katie",
  host: "Katies-Mac-mini",
  project_id: "-Users-kloughra-workspace-OpenStory",
  project_name: "OpenStory",
  branch: "feat/users-tab",
};

describe("SessionHeader", () => {
  it("renders nothing when session is null", () => {
    const { container } = render(
      <SessionHeader session={null} localUser="katie" />,
    );
    expect(container.querySelector('[data-testid="session-header"]')).toBeNull();
  });

  it("renders @user, host, project, branch when session is mine", () => {
    render(<SessionHeader session={baseSession} localUser="katie" />);
    expect(screen.getByTestId("session-header")).toBeTruthy();
    expect(screen.getByText("@katie")).toBeTruthy();
    expect(screen.getByText(/Katies-Mac-mini/)).toBeTruthy();
    expect(screen.getByText("OpenStory")).toBeTruthy();
    expect(screen.getByText("feat/users-tab")).toBeTruthy();
  });

  it("does NOT render the cross-user band when session.user matches localUser", () => {
    render(<SessionHeader session={baseSession} localUser="katie" />);
    expect(
      screen.queryByTestId("session-header-cross-user-band"),
    ).toBeNull();
    expect(
      screen.getByTestId("session-header").getAttribute("data-cross-user"),
    ).toBe("false");
  });

  it("renders the cross-user band when session.user differs from localUser", () => {
    render(
      <SessionHeader
        session={{ ...baseSession, user: "maxglassie" }}
        localUser="katie"
      />,
    );
    expect(screen.getByTestId("session-header-cross-user-band")).toBeTruthy();
    expect(
      screen.getByTestId("session-header").getAttribute("data-cross-user"),
    ).toBe("true");
    expect(screen.getByText(/Replicated from another machine/)).toBeTruthy();
    // @maxglassie shows up in BOTH the band ("viewing @maxglassie's
    // session") and the main row (the user chip), so we expect 2.
    expect(screen.getAllByText("@maxglassie")).toHaveLength(2);
  });

  it("does NOT render the band when localUser is null (still loading)", () => {
    render(<SessionHeader session={baseSession} localUser={null} />);
    expect(
      screen.queryByTestId("session-header-cross-user-band"),
    ).toBeNull();
  });

  it("does NOT render the band when session.user is null (legacy session)", () => {
    render(
      <SessionHeader session={{ ...baseSession, user: null }} localUser="katie" />,
    );
    expect(
      screen.queryByTestId("session-header-cross-user-band"),
    ).toBeNull();
  });

  it("falls back to project_id when project_name is missing", () => {
    render(
      <SessionHeader
        session={{ ...baseSession, project_name: null, project_id: "raw-pid" }}
        localUser="katie"
      />,
    );
    expect(screen.getByText("raw-pid")).toBeTruthy();
  });

  it("renders the short session id at the trailing edge", () => {
    render(<SessionHeader session={baseSession} localUser="katie" />);
    expect(screen.getByText("abcdef12")).toBeTruthy();
  });
});
