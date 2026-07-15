import { describe, it, expect } from "vitest";
import { scenario, scenarioAsync } from "../bdd";
import {
  entryMatchesEvent,
  spotlightFromEntry,
  fetchSpotlightEvent,
  SPOTLIGHT_MAX_PAGES,
} from "@/lib/event-spotlight";
import type { ConversationEntry } from "@/types/view-record";

function userEntry(id: string, text: string, seq = 1): ConversationEntry {
  return {
    entry_type: "user_message",
    id,
    seq,
    session_id: "s1",
    timestamp: "2026-07-06T14:59:27Z",
    record_type: "user_message",
    payload: { content: text },
  } as ConversationEntry;
}

function assistantEntry(id: string, text: string, seq = 2): ConversationEntry {
  return {
    entry_type: "assistant_message",
    id,
    seq,
    session_id: "s1",
    timestamp: "2026-07-06T15:00:51Z",
    record_type: "assistant_message",
    payload: { content: [{ type: "text", text }] },
  } as ConversationEntry;
}

/** A fetch stub serving conversation pages keyed by before_seq ("" = first). */
function fetchStub(pages: Record<string, { entries: ConversationEntry[]; next_before_seq?: number }>) {
  const calls: string[] = [];
  const impl = (async (url: string) => {
    calls.push(url);
    const m = /before_seq=(\d+)/.exec(url);
    const page = pages[m ? m[1]! : ""] ?? { entries: [] };
    return { ok: true, json: async () => page } as Response;
  }) as unknown as typeof fetch;
  return { impl, calls };
}

describe("entryMatchesEvent", () => {
  it("matches plain entries by top-level id and tool_roundtrips by call/result id", () => {
    scenario(
      () => ({
        plain: userEntry("e1", "hi"),
        roundtrip: {
          entry_type: "tool_roundtrip",
          call: { id: "call-1", timestamp: "2026-07-06T15:00:00Z", payload: {} },
          result: { id: "res-1", timestamp: "2026-07-06T15:00:02Z", payload: {} },
        } as unknown as ConversationEntry,
      }),
      (g) => ({
        byId: entryMatchesEvent(g.plain, "e1"),
        miss: entryMatchesEvent(g.plain, "e2"),
        byCall: entryMatchesEvent(g.roundtrip, "call-1"),
        byResult: entryMatchesEvent(g.roundtrip, "res-1"),
      }),
      (r) => {
        expect(r.byId).toBe(true);
        expect(r.miss).toBe(false);
        expect(r.byCall).toBe(true);
        expect(r.byResult).toBe(true);
      },
    );
  });
});

describe("spotlightFromEntry", () => {
  it("extracts role + FULL text from user and assistant messages (no truncation)", () => {
    const long = "a".repeat(20_000);
    scenario(
      () => ({ u: userEntry("e1", long), a: assistantEntry("e2", "**found it**") }),
      (g) => ({ u: spotlightFromEntry(g.u), a: spotlightFromEntry(g.a) }),
      (r) => {
        expect(r.u.role).toBe("user");
        expect(r.u.text).toBe(long); // never truncated
        expect(r.u.timestamp).toBe("2026-07-06T14:59:27Z");
        expect(r.a.role).toBe("assistant");
        expect(r.a.text).toBe("**found it**");
      },
    );
  });
});

describe("fetchSpotlightEvent", () => {
  it("finds an event on the first page", async () => {
    await scenarioAsync(
      () => fetchStub({ "": { entries: [userEntry("e1", "hello")] } }),
      (g) => fetchSpotlightEvent("s1", "e1", { fetchImpl: g.impl }),
      (ev) => {
        expect(ev?.role).toBe("user");
        expect(ev?.text).toBe("hello");
      },
    );
  });

  it("walks before_seq pages backward to find older events", async () => {
    await scenarioAsync(
      () =>
        fetchStub({
          "": { entries: [assistantEntry("recent", "new stuff", 900)], next_before_seq: 400 },
          "400": { entries: [userEntry("old", "the June session", 12)] },
        }),
      (g) => fetchSpotlightEvent("s1", "old", { fetchImpl: g.impl }),
      (ev) => {
        expect(ev?.role).toBe("user");
        expect(ev?.text).toBe("the June session");
      },
    );
  });

  it("returns null when history is exhausted, and bounds the walk at MAX_PAGES", async () => {
    await scenarioAsync(
      () => {
        // Every page points at another older page — an unbounded history.
        const pages: Record<string, { entries: ConversationEntry[]; next_before_seq?: number }> = {
          "": { entries: [], next_before_seq: 9999 },
        };
        for (let s = 9999; s > 9999 - SPOTLIGHT_MAX_PAGES - 5; s--) {
          pages[String(s)] = { entries: [], next_before_seq: s - 1 };
        }
        return fetchStub(pages);
      },
      async (g) => ({
        missing: await fetchSpotlightEvent("s1", "nope", { fetchImpl: g.impl }),
        calls: g.calls.length,
      }),
      (r) => {
        expect(r.missing).toBeNull();
        expect(r.calls).toBe(SPOTLIGHT_MAX_PAGES); // bounded, not unbounded
      },
    );
  });
});
