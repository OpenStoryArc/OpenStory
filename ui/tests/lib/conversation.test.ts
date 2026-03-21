import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { groupIntoTurns, totalToolCalls } from "@/lib/conversation";
import type { ConversationEntry, ToolRoundtripEntry } from "@/types/view-record";

function userMsg(text: string, ts: string = "2026-01-01T00:00:00Z"): ConversationEntry {
  return {
    entry_type: "user_message",
    id: `u-${ts}`,
    seq: 1,
    session_id: "s1",
    timestamp: ts,
    record_type: "user_message",
    payload: { text },
  } as ConversationEntry;
}

function assistantMsg(text: string, ts: string = "2026-01-01T00:00:01Z"): ConversationEntry {
  return {
    entry_type: "assistant_message",
    id: `a-${ts}`,
    seq: 2,
    session_id: "s1",
    timestamp: ts,
    record_type: "assistant_message",
    payload: { text },
  } as ConversationEntry;
}

function thinking(text: string): ConversationEntry {
  return {
    entry_type: "reasoning",
    id: "r-1",
    seq: 3,
    session_id: "s1",
    timestamp: "2026-01-01T00:00:00Z",
    record_type: "reasoning",
    payload: { text },
  } as ConversationEntry;
}

function toolRoundtrip(toolName: string): ToolRoundtripEntry {
  return {
    entry_type: "tool_roundtrip",
    call: { id: `tc-${toolName}`, record_type: "tool_call", payload: { name: toolName } } as any,
    result: { id: `tr-${toolName}`, record_type: "tool_result", payload: { output: "ok" } } as any,
  };
}

// ── groupIntoTurns ──────────────────────

describe("groupIntoTurns", () => {
  it("returns empty for no entries", () => {
    scenario(
      () => [] as ConversationEntry[],
      (entries) => groupIntoTurns(entries),
      (turns) => expect(turns).toEqual([]),
    );
  });

  it("single turn: prompt → response", () => {
    const entries: ConversationEntry[] = [
      userMsg("Fix the bug"),
      assistantMsg("Done!"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.prompt).toBe("Fix the bug");
        expect(turns[0]!.response).toBe("Done!");
        expect(turns[0]!.toolCalls).toHaveLength(0);
        expect(turns[0]!.thinking).toBeNull();
      },
    );
  });

  it("turn with thinking and tools", () => {
    const entries: ConversationEntry[] = [
      userMsg("Fix the bug"),
      thinking("Let me think about this..."),
      toolRoundtrip("Read"),
      toolRoundtrip("Edit"),
      assistantMsg("Fixed!"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.thinking).toBe("Let me think about this...");
        expect(turns[0]!.toolCalls).toHaveLength(2);
        expect(turns[0]!.response).toBe("Fixed!");
      },
    );
  });

  it("multiple turns", () => {
    const entries: ConversationEntry[] = [
      userMsg("First question", "2026-01-01T00:00:00Z"),
      assistantMsg("First answer", "2026-01-01T00:00:01Z"),
      userMsg("Second question", "2026-01-01T00:00:02Z"),
      toolRoundtrip("Bash"),
      assistantMsg("Second answer", "2026-01-01T00:00:03Z"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(2);
        expect(turns[0]!.prompt).toBe("First question");
        expect(turns[0]!.toolCalls).toHaveLength(0);
        expect(turns[1]!.prompt).toBe("Second question");
        expect(turns[1]!.toolCalls).toHaveLength(1);
      },
    );
  });

  it("turn with no prompt (starts with tools)", () => {
    const entries: ConversationEntry[] = [
      toolRoundtrip("Read"),
      assistantMsg("Here's what I found"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.prompt).toBeNull();
        expect(turns[0]!.toolCalls).toHaveLength(1);
        expect(turns[0]!.response).toBe("Here's what I found");
      },
    );
  });

  it("turn with no response (just tools)", () => {
    const entries: ConversationEntry[] = [
      userMsg("Do something"),
      toolRoundtrip("Bash"),
      toolRoundtrip("Bash"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.prompt).toBe("Do something");
        expect(turns[0]!.response).toBeNull();
        expect(turns[0]!.toolCalls).toHaveLength(2);
      },
    );
  });

  it("multiple thinking blocks concatenate", () => {
    const entries: ConversationEntry[] = [
      userMsg("Think hard"),
      thinking("Part 1"),
      thinking("Part 2"),
      assistantMsg("Thought about it"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.thinking).toBe("Part 1\nPart 2");
      },
    );
  });

  it("skips token_usage entries", () => {
    const entries: ConversationEntry[] = [
      userMsg("Hello"),
      { entry_type: "token_usage", id: "t1", seq: 1, session_id: "s1", timestamp: "2026-01-01T00:00:00Z", record_type: "token_usage", payload: { input_tokens: 100, output_tokens: 50 } } as ConversationEntry,
      assistantMsg("Hi"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.prompt).toBe("Hello");
        expect(turns[0]!.response).toBe("Hi");
      },
    );
  });
});

// ── totalToolCalls ──────────────────────

describe("totalToolCalls", () => {
  it("zero for empty turns", () => {
    scenario(
      () => [] as ReturnType<typeof groupIntoTurns>,
      (turns) => totalToolCalls(turns),
      (count) => expect(count).toBe(0),
    );
  });

  it("sums across turns", () => {
    const turns = groupIntoTurns([
      userMsg("1"), toolRoundtrip("A"), toolRoundtrip("B"), assistantMsg("r1"),
      userMsg("2"), toolRoundtrip("C"), assistantMsg("r2"),
    ]);
    scenario(
      () => turns,
      (t) => totalToolCalls(t),
      (count) => expect(count).toBe(3),
    );
  });
});

describe("groupIntoTurns — edge cases", () => {
  it("thinking block without preceding user_message starts implicit turn", () => {
    const entries: ConversationEntry[] = [
      thinking("Pondering..."),
      assistantMsg("Done thinking"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.prompt).toBeNull();
        expect(turns[0]!.thinking).toBe("Pondering...");
        expect(turns[0]!.response).toBe("Done thinking");
      },
    );
  });

  it("assistant_message without preceding user_message starts implicit turn", () => {
    const entries: ConversationEntry[] = [
      assistantMsg("Unprompted response"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.prompt).toBeNull();
        expect(turns[0]!.response).toBe("Unprompted response");
      },
    );
  });

  it("multiple assistant_messages in one turn — last one wins", () => {
    const entries: ConversationEntry[] = [
      userMsg("question"),
      assistantMsg("first draft", "2026-01-01T00:00:01Z"),
      assistantMsg("revised answer", "2026-01-01T00:00:02Z"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.response).toBe("revised answer");
        expect(turns[0]!.responseTimestamp).toBe("2026-01-01T00:00:02Z");
      },
    );
  });

  it("system entry is skipped (default branch)", () => {
    const entries: ConversationEntry[] = [
      userMsg("Hello"),
      { entry_type: "system", id: "sys1", seq: 2, session_id: "s1", timestamp: "2026-01-01T00:00:00Z", record_type: "system_event", payload: { subtype: "hook" } } as ConversationEntry,
      assistantMsg("Hi"),
    ];
    scenario(
      () => entries,
      (e) => groupIntoTurns(e),
      (turns) => {
        expect(turns).toHaveLength(1);
        expect(turns[0]!.prompt).toBe("Hello");
        expect(turns[0]!.response).toBe("Hi");
      },
    );
  });
});
