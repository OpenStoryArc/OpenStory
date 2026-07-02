import { describe, it, expect } from "vitest";
import { conversationEntryMatches, facetCounts } from "@/lib/conversation-facets";
import type { ConversationEntry } from "@/types/view-record";

const user = { entry_type: "user_message" } as ConversationEntry;
const assistant = { entry_type: "assistant_message" } as ConversationEntry;
const thinking = { entry_type: "reasoning" } as ConversationEntry;
const tool = (name: string, command?: string, output?: string, is_error?: boolean): ConversationEntry =>
  ({
    entry_type: "tool_roundtrip",
    call: { payload: { name, raw_input: command ? { command } : {} } },
    result: output != null || is_error ? { payload: { output, is_error } } : null,
  }) as unknown as ConversationEntry;

describe("conversationEntryMatches — the Live facets, for paired conversation entries", () => {
  it("messages → conversation", () => {
    expect(conversationEntryMatches(user, "conversation")).toBe(true);
    expect(conversationEntryMatches(assistant, "conversation")).toBe(true);
    expect(conversationEntryMatches(user, "code")).toBe(false);
  });
  it("Write/Edit/Read → code; Bash → commands", () => {
    expect(conversationEntryMatches(tool("Write"), "code")).toBe(true);
    expect(conversationEntryMatches(tool("Bash", "ls"), "commands")).toBe(true);
    expect(conversationEntryMatches(tool("Bash", "ls"), "code")).toBe(false);
  });
  it("git bash → git; test command → tests", () => {
    expect(conversationEntryMatches(tool("Bash", "git commit -m x"), "git")).toBe(true);
    expect(conversationEntryMatches(tool("Bash", "cargo test"), "tests")).toBe(true);
  });
  it("errored tool result → errors", () => {
    expect(conversationEntryMatches(tool("Bash", "boom", "err", true), "errors")).toBe(true);
  });
  it("reasoning → thinking; Agent → agents; plan mode → plans", () => {
    expect(conversationEntryMatches(thinking, "thinking")).toBe(true);
    expect(conversationEntryMatches(tool("Agent"), "agents")).toBe(true);
    expect(conversationEntryMatches(tool("ExitPlanMode"), "plans")).toBe(true);
  });
  it("all → always true", () => {
    expect(conversationEntryMatches(tool("Bash", "ls"), "all")).toBe(true);
  });
});

describe("facetCounts — per-facet counts for the pills", () => {
  it("counts each facet; all = total", () => {
    const entries = [user, assistant, tool("Write"), tool("Bash", "git status"), thinking];
    const c = facetCounts(entries);
    expect(c.all).toBe(5);
    expect(c.conversation).toBe(2);
    expect(c.code).toBe(1);
    expect(c.git).toBe(1);
    expect(c.thinking).toBe(1);
  });
});
