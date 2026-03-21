import { describe, it, expect } from "vitest";
import { eventCategory, eventLabel } from "@/types/cloud-event";
import type { ArcEventType, EventCategory } from "@/types/cloud-event";

// ── eventCategory: complete boundary table ──────────────────────────
//
// Every row is: [type, subtype, expected category]
// This table IS the specification. If a new event type is added,
// add a row here. If a classification changes, the table breaks.

const CATEGORY_BOUNDARY_TABLE: [ArcEventType, string | undefined, EventCategory | null][] = [
  // ── Unified io.arc.event subtypes ──
  ["io.arc.event", "message.user.prompt",      "prompts"],
  ["io.arc.event", "message.user.tool_result",  "tools"],
  ["io.arc.event", "message.assistant.text",     "responses"],
  ["io.arc.event", "message.assistant.tool_use", "tools"],
  ["io.arc.event", "message.assistant.thinking", "responses"],
  ["io.arc.event", "system.error",               "errors"],
  ["io.arc.event", "file.snapshot",              "files"],
  ["io.arc.event", "file.edit",                  "files"],
  ["io.arc.event", "system.turn.complete",       null],
  ["io.arc.event", "system.compact",             null],
  ["io.arc.event", "system.hook",                null],
  ["io.arc.event", "system.session.start",       null],
  ["io.arc.event", "system.session.end",         null],
  ["io.arc.event", "progress.bash",              null],
  ["io.arc.event", "progress.agent",             null],
  ["io.arc.event", "queue.enqueue",              null],
  ["io.arc.event", "queue.dequeue",              null],
  ["io.arc.event", undefined,                    null],

  // ── Legacy hook-style types ──
  ["io.arc.prompt.submit",     undefined, "prompts"],
  ["io.arc.response.complete", undefined, "responses"],
  ["io.arc.tool.call",         undefined, "tools"],
  ["io.arc.tool.result",       undefined, "tools"],
  ["io.arc.error",             undefined, "errors"],
  ["io.arc.file.edit",         undefined, "files"],
  ["io.arc.session.start",     undefined, null],
  ["io.arc.session.end",       undefined, null],

  // ── Legacy transcript-style types ──
  ["io.arc.transcript.user",      "text",         "prompts"],
  ["io.arc.transcript.user",      "tool_result",  "tools"],
  ["io.arc.transcript.user",      undefined,      "prompts"],
  ["io.arc.transcript.assistant", "text",         "responses"],
  ["io.arc.transcript.assistant", "tool_use",     "tools"],
  ["io.arc.transcript.assistant", "thinking",     "responses"],
  ["io.arc.transcript.system",    "api_error",    "errors"],
  ["io.arc.transcript.system",    "turn_duration", null],
  ["io.arc.transcript.system",    "compact",       null],
  ["io.arc.transcript.progress",  undefined,       null],
  ["io.arc.transcript.snapshot",  undefined,       null],
  ["io.arc.transcript.queue",     undefined,       null],
];

describe("eventCategory — boundary table", () => {
  it.each(CATEGORY_BOUNDARY_TABLE)(
    "eventCategory(%s, %s) → %s",
    (type, subtype, expected) => {
      expect(eventCategory(type, subtype)).toBe(expected);
    },
  );
});

// ── eventLabel: complete boundary table ─────────────────────────────
//
// Every row is: [type, subtype, expected label]

const LABEL_BOUNDARY_TABLE: [ArcEventType, string | undefined, string][] = [
  // ── Unified subtypes → known labels ──
  ["io.arc.event", "message.user.prompt",        "Prompt"],
  ["io.arc.event", "message.user.tool_result",    "Result"],
  ["io.arc.event", "message.assistant.text",       "Response"],
  ["io.arc.event", "message.assistant.tool_use",   "Tool Use"],
  ["io.arc.event", "message.assistant.thinking",   "Thinking"],
  ["io.arc.event", "system.turn.complete",         "Complete"],
  ["io.arc.event", "system.error",                 "Error"],
  ["io.arc.event", "system.compact",               "Compact"],
  ["io.arc.event", "system.hook",                  "Hook"],
  ["io.arc.event", "system.session.start",         "Start"],
  ["io.arc.event", "system.session.end",           "End"],
  ["io.arc.event", "progress.bash",                "Bash"],
  ["io.arc.event", "progress.agent",               "Agent"],
  ["io.arc.event", "progress.hook",                "Hook"],
  ["io.arc.event", "file.snapshot",                "Snapshot"],
  ["io.arc.event", "file.edit",                    "Edit"],
  ["io.arc.event", "queue.enqueue",                "Enqueue"],
  ["io.arc.event", "queue.dequeue",                "Dequeue"],

  // ── Unified subtypes → unknown → last segment ──
  ["io.arc.event", "custom.unknown.thing",  "thing"],
  ["io.arc.event", "some.new.subtype",      "subtype"],

  // ── Unified with no subtype → raw type ──
  ["io.arc.event", undefined, "io.arc.event"],

  // ── Legacy hook types ──
  ["io.arc.session.start",     undefined, "Start"],
  ["io.arc.session.end",       undefined, "End"],
  ["io.arc.prompt.submit",     undefined, "Prompt"],
  ["io.arc.response.complete", undefined, "Response"],
  ["io.arc.tool.call",         undefined, "Call"],
  ["io.arc.tool.result",       undefined, "Result"],
  ["io.arc.file.edit",         undefined, "Edit"],
  ["io.arc.error",             undefined, "Error"],

  // ── Legacy transcript types ──
  ["io.arc.transcript.user",      undefined, "User"],
  ["io.arc.transcript.assistant",  undefined, "Assistant"],
  ["io.arc.transcript.system",     undefined, "System"],
  ["io.arc.transcript.progress",   undefined, "Progress"],
  ["io.arc.transcript.snapshot",   undefined, "Snapshot"],
  ["io.arc.transcript.queue",      undefined, "Queue"],
];

describe("eventLabel — boundary table", () => {
  it.each(LABEL_BOUNDARY_TABLE)(
    "eventLabel(%s, %s) → %s",
    (type, subtype, expected) => {
      expect(eventLabel(type, subtype)).toBe(expected);
    },
  );
});
