import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import {
  isGitBashEvent,
  eventColor,
  eventSummary,
  shortenCommand,
  basename,
  truncate,
} from "@/lib/event-transforms";
import type { CloudEvent } from "@/types/cloud-event";

// ---------------------------------------------------------------------------
// Helper: minimal CloudEvent factory
// ---------------------------------------------------------------------------

function makeEvent(overrides: Omit<Partial<CloudEvent>, "data" | "type"> & { type: string; data?: Record<string, unknown> }): CloudEvent {
  return {
    id: "test-id",
    source: "test",
    specversion: "1.0",
    time: "2026-01-01T00:00:00Z",
    datacontenttype: "application/json",
    data: { seq: 1, ts: 0, parent_seq: null, ...(overrides.data ?? {}) },
    ...overrides,
  } as unknown as CloudEvent;
}

// ---------------------------------------------------------------------------
// shortenCommand
// ---------------------------------------------------------------------------

describe("shortenCommand", () => {
  it.each([
    ["cd /home/user/project && cargo test", "cargo test"],
    ['cd "C:\\Users\\test" && npm run build', "npm run build"],
    ["plain command", "plain command"],
    ["", ""],
    ["cd /path && git status", "git status"],
    ["cd /very/deep/nested/path && ls -la", "ls -la"],
  ])("shortenCommand(%j) => %j", (input, expected) => {
    expect(shortenCommand(input)).toBe(expected);
  });
});

// ---------------------------------------------------------------------------
// basename
// ---------------------------------------------------------------------------

describe("basename", () => {
  it.each([
    ["/home/user/project/src/main.rs", "main.rs"],
    ["C:\\Users\\test\\file.txt", "file.txt"],
    ["bare-filename", "bare-filename"],
    ["path/to/deep/file.js", "file.js"],
    ["/single-level", "single-level"],
  ])("basename(%j) => %j", (input, expected) => {
    expect(basename(input)).toBe(expected);
  });
});

// ---------------------------------------------------------------------------
// truncate
// ---------------------------------------------------------------------------

describe("truncate", () => {
  it.each([
    ["short", 10, "short"],
    ["exactly10!", 10, "exactly10!"],
    ["this is over the limit", 10, "this is o\u2026"],
    ["", 5, ""],
    ["ab", 1, "\u2026"],
    ["hello world this is long", 5, "hell\u2026"],
  ] as [string, number, string][])(
    "truncate(%j, %d) => %j",
    (input, max, expected) => {
      expect(truncate(input, max)).toBe(expected);
    },
  );
});

// ---------------------------------------------------------------------------
// isGitBashEvent
// ---------------------------------------------------------------------------

describe("isGitBashEvent", () => {
  it.each([
    [
      "unified tool_use + Bash + git command",
      makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.tool_use",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "git status" } },
      }),
      true,
    ],
    [
      "unified tool_use + Bash + non-git command",
      makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.tool_use",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "cargo test" } },
      }),
      false,
    ],
    [
      "unified tool_use + Read + git-like",
      makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.tool_use",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Read", args: { command: "git status" } },
      }),
      false,
    ],
    [
      "legacy io.arc.tool.call + Bash + git",
      makeEvent({
        type: "io.arc.tool.call",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "git log" } },
      }),
      true,
    ],
    [
      "legacy io.arc.tool.call + Bash + non-git",
      makeEvent({
        type: "io.arc.tool.call",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "npm install" } },
      }),
      false,
    ],
    [
      "other type + Bash + git => false",
      makeEvent({
        type: "io.arc.prompt.submit",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "git push" } },
      }),
      false,
    ],
    [
      "unified tool_use with no tool field",
      makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.tool_use",
        data: { seq: 1, ts: 0, parent_seq: null },
      }),
      false,
    ],
  ] as [string, CloudEvent, boolean][])(
    "%s => %s",
    (_label, event, expected) => {
      expect(isGitBashEvent(event)).toBe(expected);
    },
  );
});

// ---------------------------------------------------------------------------
// eventColor — unified subtypes
// ---------------------------------------------------------------------------

describe("eventColor", () => {
  describe("unified subtypes", () => {
    it.each([
      ["message.user.prompt", "#7aa2f7"],
      ["message.user.tool_result", "#2ac3de"],
      ["message.assistant.text", "#bb9af7"],
      ["message.assistant.tool_use", "#2ac3de"],
      ["message.assistant.thinking", "#9ece6a"],
      ["system.turn.complete", "#565f89"],
      ["system.error", "#f7768e"],
      ["system.compact", "#565f89"],
      ["system.hook", "#565f89"],
      ["system.session.start", "#9ece6a"],
      ["system.session.end", "#565f89"],
      ["progress.bash", "#e0af68"],
      ["progress.agent", "#e0af68"],
      ["progress.hook", "#e0af68"],
      ["file.snapshot", "#565f89"],
      ["file.edit", "#e0af68"],
      ["queue.enqueue", "#565f89"],
      ["queue.dequeue", "#565f89"],
    ])("subtype %s => %s", (subtype, color) => {
      const event = makeEvent({ type: "io.arc.event", subtype });
      expect(eventColor(event)).toBe(color);
    });
  });

  describe("legacy types", () => {
    it.each([
      ["io.arc.session.start", "#9ece6a"],
      ["io.arc.session.end", "#565f89"],
      ["io.arc.prompt.submit", "#7aa2f7"],
      ["io.arc.response.complete", "#bb9af7"],
      ["io.arc.tool.call", "#2ac3de"],
      ["io.arc.tool.result", "#2ac3de"],
      ["io.arc.file.edit", "#e0af68"],
      ["io.arc.error", "#f7768e"],
      ["io.arc.transcript.user", "#7aa2f7"],
      ["io.arc.transcript.assistant", "#bb9af7"],
      ["io.arc.transcript.system", "#565f89"],
      ["io.arc.transcript.progress", "#e0af68"],
      ["io.arc.transcript.snapshot", "#565f89"],
      ["io.arc.transcript.queue", "#565f89"],
    ] as [string, string][])("legacy %s => %s", (type, color) => {
      const event = makeEvent({ type });
      expect(eventColor(event)).toBe(color);
    });
  });

  it("returns fallback for unknown unified subtype", () => {
    const event = makeEvent({ type: "io.arc.event", subtype: "totally.unknown" });
    expect(eventColor(event)).toBe("#565f89");
  });

  it("returns fallback for unknown legacy type", () => {
    const event = makeEvent({ type: "io.arc.unknown.type" as any });
    expect(eventColor(event)).toBe("#565f89");
  });

  it("returns git risk color for safe git command", () => {
    scenario(
      () =>
        makeEvent({
          type: "io.arc.event",
          subtype: "message.assistant.tool_use",
          data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "git status" } },
        }),
      (event) => eventColor(event),
      (color) => expect(color).toBe("#565f89"), // safe risk color
    );
  });

  it("returns git risk color for destructive git command", () => {
    scenario(
      () =>
        makeEvent({
          type: "io.arc.event",
          subtype: "message.assistant.tool_use",
          data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "git push --force origin main" } },
        }),
      (event) => eventColor(event),
      (color) => expect(color).toBe("#f7768e"), // destructive risk color
    );
  });
});

// ---------------------------------------------------------------------------
// eventSummary — unified io.arc.event subtypes
// ---------------------------------------------------------------------------

describe("eventSummary", () => {
  describe("unified subtypes", () => {
    it("message.user.prompt with text", () => {
      scenario(
        () =>
          makeEvent({
            type: "io.arc.event",
            subtype: "message.user.prompt",
            data: { seq: 1, ts: 0, parent_seq: null, text: "Hello, world!" },
          }),
        (event) => eventSummary(event),
        (summary) => expect(summary).toBe("Hello, world!"),
      );
    });

    it("message.user.prompt without text", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "message.user.prompt",
        data: { seq: 1, ts: 0, parent_seq: null },
      });
      expect(eventSummary(event)).toBe("User message");
    });

    it("message.user.tool_result", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "message.user.tool_result",
      });
      expect(eventSummary(event)).toBe("Tool result");
    });

    it("message.assistant.tool_use with tool name and command", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.tool_use",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "cargo test" } },
      });
      expect(eventSummary(event)).toBe("Bash: cargo test");
    });

    it("message.assistant.tool_use with tool name and file_path", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.tool_use",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Read", args: { file_path: "/src/main.rs" } },
      });
      expect(eventSummary(event)).toBe("Read: main.rs");
    });

    it("message.assistant.tool_use with no detail", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.tool_use",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Read" },
      });
      expect(eventSummary(event)).toBe("Read");
    });

    it("message.assistant.thinking", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.thinking",
      });
      expect(eventSummary(event)).toBe("Thinking...");
    });

    it("message.assistant.text with text", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.text",
        data: { seq: 1, ts: 0, parent_seq: null, text: "Here is my response" },
      });
      expect(eventSummary(event)).toBe("Here is my response");
    });

    it("message.assistant.text without text", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "message.assistant.text",
        data: { seq: 1, ts: 0, parent_seq: null },
      });
      expect(eventSummary(event)).toBe("Assistant response");
    });

    it("system.turn.complete with duration_ms", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "system.turn.complete",
        data: { seq: 1, ts: 0, parent_seq: null, duration_ms: 5200 },
      });
      expect(eventSummary(event)).toBe("Turn completed (5.2s)");
    });

    it("system.turn.complete without duration_ms", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "system.turn.complete",
        data: { seq: 1, ts: 0, parent_seq: null },
      });
      expect(eventSummary(event)).toBe("Turn completed");
    });

    it("system.error with message", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "system.error",
        data: { seq: 1, ts: 0, parent_seq: null, message: "Something went wrong" },
      });
      expect(eventSummary(event)).toBe("Something went wrong");
    });

    it("system.hook", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "system.hook",
      });
      expect(eventSummary(event)).toBe("Hook summary");
    });

    it("system.compact", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "system.compact",
      });
      expect(eventSummary(event)).toBe("Context compacted");
    });

    it("progress.bash", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "progress.bash",
      });
      expect(eventSummary(event)).toBe("bash");
    });

    it("progress.agent", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "progress.agent",
      });
      expect(eventSummary(event)).toBe("agent");
    });

    it("file.snapshot", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "file.snapshot",
      });
      expect(eventSummary(event)).toBe("File history snapshot");
    });

    it("file.edit with operation and path", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "file.edit",
        data: { seq: 1, ts: 0, parent_seq: null, operation: "modify", path: "/src/lib.rs" },
      });
      expect(eventSummary(event)).toBe("modify: lib.rs");
    });

    it("queue.enqueue with operation", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "queue.enqueue",
        data: { seq: 1, ts: 0, parent_seq: null, operation: "enqueue" },
      });
      expect(eventSummary(event)).toBe("enqueue");
    });

    it("unknown subtype falls back to subtype string", () => {
      const event = makeEvent({
        type: "io.arc.event",
        subtype: "custom.new.thing",
      });
      expect(eventSummary(event)).toBe("custom.new.thing");
    });
  });

  describe("legacy types", () => {
    it("io.arc.session.start with model", () => {
      const event = makeEvent({
        type: "io.arc.session.start",
        data: { seq: 1, ts: 0, parent_seq: null, model: "claude-3" },
      });
      expect(eventSummary(event)).toBe("Session started (claude-3)");
    });

    it("io.arc.session.start without model", () => {
      const event = makeEvent({
        type: "io.arc.session.start",
        data: { seq: 1, ts: 0, parent_seq: null },
      });
      expect(eventSummary(event)).toBe("Session started");
    });

    it("io.arc.tool.call with tool and command", () => {
      const event = makeEvent({
        type: "io.arc.tool.call",
        subtype: "Bash",
        data: { seq: 1, ts: 0, parent_seq: null, tool: "Bash", args: { command: "ls -la" } },
      });
      expect(eventSummary(event)).toBe("Bash: ls -la");
    });

    it("io.arc.error with message", () => {
      const event = makeEvent({
        type: "io.arc.error",
        data: { seq: 1, ts: 0, parent_seq: null, message: "File not found" },
      });
      expect(eventSummary(event)).toBe("File not found");
    });

    it("io.arc.transcript.snapshot", () => {
      const event = makeEvent({
        type: "io.arc.transcript.snapshot",
      });
      expect(eventSummary(event)).toBe("File history snapshot");
    });

    it("unknown legacy type falls back to type string", () => {
      const event = makeEvent({
        type: "io.arc.unknown.thing" as any,
      });
      expect(eventSummary(event)).toBe("io.arc.unknown.thing");
    });
  });
});

describe("eventSummary — legacy transcript paths", () => {
  it("io.arc.transcript.user extracts text from raw.message.content string", () => {
    const event = makeEvent({
      type: "io.arc.transcript.user",
      data: { raw: { message: { content: "direct text" } } },
    });
    expect(eventSummary(event)).toBe("direct text");
  });

  it("io.arc.transcript.user extracts text from raw.message.content array", () => {
    const event = makeEvent({
      type: "io.arc.transcript.user",
      data: {
        raw: { message: { content: [{ type: "text", text: "from blocks" }] } },
      },
    });
    expect(eventSummary(event)).toBe("from blocks");
  });

  it("io.arc.transcript.user with no text returns fallback", () => {
    const event = makeEvent({
      type: "io.arc.transcript.user",
      data: { raw: { message: { content: [] } } },
    });
    expect(eventSummary(event)).toBe("User message");
  });

  it("io.arc.transcript.assistant tool_use extracts tool names from raw blocks", () => {
    const event = makeEvent({
      type: "io.arc.transcript.assistant",
      subtype: "tool_use",
      data: {
        raw: {
          message: {
            content: [
              { type: "tool_use", name: "Read" },
              { type: "tool_use", name: "Edit" },
            ],
          },
        },
      },
    });
    expect(eventSummary(event)).toBe("Read, Edit");
  });

  it("io.arc.transcript.system with turn_duration and duration_ms", () => {
    const event = makeEvent({
      type: "io.arc.transcript.system",
      subtype: "turn_duration",
      data: { duration_ms: 125000 },
    });
    expect(eventSummary(event)).toContain("Turn completed");
    expect(eventSummary(event)).toContain("2m 5s");
  });

  it("io.arc.transcript.progress with no subtype uses data.progress_type", () => {
    const event = makeEvent({
      type: "io.arc.transcript.progress",
      data: { progress_type: "bash" },
    });
    expect(eventSummary(event)).toBe("bash");
  });

  it("io.arc.transcript.queue uses data.operation", () => {
    const event = makeEvent({
      type: "io.arc.transcript.queue",
      data: { operation: "enqueue" },
    });
    expect(eventSummary(event)).toBe("enqueue");
  });
});

describe("eventSummary — toolCallDetail branches", () => {
  it("tool_call with args.pattern shows pattern", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "message.assistant.tool_use",
      data: { tool: "Grep", args: { pattern: "TODO" } },
    });
    expect(eventSummary(event)).toContain("TODO");
  });

  it("tool_call with args.query shows query", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "message.assistant.tool_use",
      data: { tool: "WebSearch", args: { query: "rust async" } },
    });
    expect(eventSummary(event)).toContain("rust async");
  });

  it("tool_call with args.url shows url", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "message.assistant.tool_use",
      data: { tool: "WebFetch", args: { url: "https://example.com" } },
    });
    expect(eventSummary(event)).toContain("https://example.com");
  });

  it("tool_call with no recognized args shows tool name only", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "message.assistant.tool_use",
      data: { tool: "Agent", args: { prompt: "do something" } },
    });
    // args.prompt is not one of the recognized keys in toolCallDetail
    // So it should just show the tool name
    expect(eventSummary(event)).toBe("Agent");
  });

  it("tool_call with no args shows tool name", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "message.assistant.tool_use",
      data: { tool: "Agent" },
    });
    expect(eventSummary(event)).toBe("Agent");
  });
});

describe("eventSummary — formatDurationCompact", () => {
  it("sub-second duration", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "system.turn.complete",
      data: { duration_ms: 450 },
    });
    expect(eventSummary(event)).toBe("Turn completed (450ms)");
  });

  it("seconds duration", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "system.turn.complete",
      data: { duration_ms: 5500 },
    });
    expect(eventSummary(event)).toBe("Turn completed (5.5s)");
  });

  it("minutes with seconds", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "system.turn.complete",
      data: { duration_ms: 125000 },
    });
    expect(eventSummary(event)).toBe("Turn completed (2m 5s)");
  });

  it("exact minutes no seconds", () => {
    const event = makeEvent({
      type: "io.arc.event",
      subtype: "system.turn.complete",
      data: { duration_ms: 120000 },
    });
    expect(eventSummary(event)).toBe("Turn completed (2m)");
  });
});
