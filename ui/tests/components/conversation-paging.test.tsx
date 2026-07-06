/** ConversationView loads one page of recent history (bounded window),
 *  shows a shaped skeleton while fetching, and loads older pages on demand
 *  through the next_before_seq cursor — never the whole session up front.
 *
 *  (Entry text itself is virtualized and jsdom reports zero heights, so
 *  these specs assert the paging contract: request URLs and the cursor
 *  button's lifecycle.) */

import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { ConversationView } from "@/components/conversation/ConversationView";

function entry(text: string, seq: number) {
  return {
    entry_type: "user_message",
    timestamp: `2026-07-04T10:00:${String(seq).padStart(2, "0")}Z`,
    payload: { content: text },
  };
}

afterEach(() => vi.unstubAllGlobals());

describe("when a session's conversation loads", () => {
  it("should show a shaped skeleton, fetch one bounded page, and offer older history", async () => {
    const urls: string[] = [];
    let resolve!: (v: unknown) => void;
    vi.stubGlobal(
      "fetch",
      vi.fn((url: string) => {
        urls.push(String(url));
        return new Promise((r) => {
          resolve = r;
        });
      }),
    );

    render(<ConversationView sessionId="s-page" />);
    // Shaped skeleton, not a bare text line.
    expect(screen.getByTestId("conversation-loading")).toBeInTheDocument();

    resolve({
      ok: true,
      json: async () => ({
        entries: [entry("recent question", 40)],
        next_before_seq: 40,
      }),
    });

    await waitFor(() => expect(screen.getByTestId("load-older")).toBeInTheDocument());
    expect(urls).toHaveLength(1);
    expect(urls[0]).toContain("/api/sessions/s-page/conversation?limit=");
    expect(urls[0]).not.toContain("before_seq");
    expect(screen.queryByTestId("conversation-loading")).toBeNull();
  });

  it("should walk the cursor and drop the button when history is exhausted", async () => {
    const urls: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn((url: string) => {
        urls.push(String(url));
        const isOlder = String(url).includes("before_seq");
        return Promise.resolve({
          ok: true,
          json: async () =>
            isOlder
              ? { entries: [entry("older question", 10)] } // no cursor → exhausted
              : { entries: [entry("recent question", 40)], next_before_seq: 40 },
        });
      }),
    );

    render(<ConversationView sessionId="s-page" />);
    await waitFor(() => expect(screen.getByTestId("load-older")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("load-older"));
    // The older page carried no cursor → history exhausted → button gone.
    await waitFor(() => expect(screen.queryByTestId("load-older")).toBeNull());

    expect(urls).toHaveLength(2);
    expect(urls[1]).toContain("before_seq=40");
  });
});
