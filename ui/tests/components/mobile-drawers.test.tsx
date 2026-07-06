import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ExploreView } from "@/components/explore/ExploreView";
import { StoryView } from "@/components/story/StoryView";

/** Phones get drawers, not slivers: at narrow widths the Explore sidebar
 *  starts closed behind a ☰ toggle and closes itself after a selection, so
 *  the detail pane owns the screen. (Story's sidebar reuses its existing
 *  open/close state with a width-aware default — covered by its own suite.) */

function setWidth(w: number) {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: w });
}

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const u = String(url);
      const body = () => {
        if (/\/api\/sessions(\?|$)/.test(u))
          return { sessions: [{ session_id: "s1", start_time: "2026-06-10T09:00:00.000Z", event_count: 5, status: "completed", label: "hello" }] };
        if (u.includes("/synopsis")) return null;
        return [];
      };
      return { ok: true, statusText: "OK", status: 200, json: async () => body() };
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  setWidth(1024);
});

describe("when Explore renders on a phone-width viewport", () => {
  it("should start with the sidebar drawer closed and open it from the ☰ toggle", () => {
    setWidth(390);
    render(<ExploreView route={{ view: "explore" }} onNavigate={vi.fn()} />);
    const drawer = screen.getByTestId("explore-drawer");
    expect(drawer.getAttribute("data-state")).toBe("closed");
    fireEvent.click(screen.getByTestId("sidebar-toggle"));
    expect(drawer.getAttribute("data-state")).toBe("open");
  });

  it("should close the drawer after selecting a session so the detail owns the screen", async () => {
    setWidth(390);
    const onNavigate = vi.fn();
    render(<ExploreView route={{ view: "explore" }} onNavigate={onNavigate} />);
    fireEvent.click(screen.getByTestId("sidebar-toggle"));
    const row = await screen.findByTestId("explore-session-s1");
    fireEvent.click(row);
    expect(onNavigate).toHaveBeenCalled();
    expect(screen.getByTestId("explore-drawer").getAttribute("data-state")).toBe("closed");
  });
});

describe("when Explore renders on a desktop viewport", () => {
  it("should keep the sidebar statically open", () => {
    setWidth(1280);
    render(<ExploreView route={{ view: "explore" }} onNavigate={vi.fn()} />);
    expect(screen.getByTestId("explore-drawer").getAttribute("data-state")).toBe("open");
  });
});

describe("when Story renders on a phone-width viewport", () => {
  it("should start with its sidebar closed (the ☰ opener visible) so the feed owns the screen", () => {
    setWidth(390);
    render(<StoryView livePatterns={[]} selectedSession={null} onSelectSession={vi.fn()} />);
    expect(screen.getByTitle("Open sidebar")).toBeTruthy();
  });
});
