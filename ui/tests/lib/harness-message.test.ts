import { describe, it, expect } from "vitest";
import { scenario } from "../bdd";
import { classifyHarnessMessage, cleanHarnessPreview, isHarnessMessage } from "@/lib/harness-message";

describe("classifyHarnessMessage", () => {
  it("parses a slash-command invocation with args", () => {
    scenario(
      () => "<command-message>loop</command-message>\n<command-name>loop</command-name>\n<command-args>please improve the UI</command-args>",
      (t) => classifyHarnessMessage(t),
      (m) => {
        expect(m.kind).toBe("slash_command");
        if (m.kind === "slash_command") {
          expect(m.command).toBe("loop");
          expect(m.args).toBe("please improve the UI");
        }
      },
    );
  });

  it("recovers the command name from a truncated 50-char label", () => {
    scenario(
      // exactly the shape the server label truncation produces
      () => "<command-message>loop</command-message>\n<command-n",
      (t) => cleanHarnessPreview(t),
      (preview) => expect(preview).toBe("/loop"),
    );
  });

  it("parses a task notification's status and summary", () => {
    scenario(
      () => "<task-notification>\n<task-id>abc123</task-id>\n<status>completed</status>\n<summary>Agent \"Boot audit\" finished</summary>\n</task-notification>",
      (t) => classifyHarnessMessage(t),
      (m) => {
        expect(m.kind).toBe("task_notification");
        if (m.kind === "task_notification") {
          expect(m.status).toBe("completed");
          expect(m.summary).toContain("Boot audit");
          expect(m.taskId).toBe("abc123");
        }
      },
    );
  });

  it("classifies system reminders and local stdout", () => {
    expect(classifyHarnessMessage("<system-reminder>do the thing</system-reminder>").kind).toBe("system_reminder");
    expect(classifyHarnessMessage("<local-command-stdout>hello</local-command-stdout>").kind).toBe("local_stdout");
  });

  it("leaves ordinary human messages untouched", () => {
    scenario(
      () => "Can you fix the login bug?",
      (t) => ({ cls: classifyHarnessMessage(t), harness: isHarnessMessage(t) }),
      (r) => {
        expect(r.cls.kind).toBe("plain");
        expect(r.harness).toBe(false);
        if (r.cls.kind === "plain") expect(r.cls.text).toBe("Can you fix the login bug?");
      },
    );
  });
});

describe("cleanHarnessPreview", () => {
  it("humanizes each wrapper kind into a readable one-liner", () => {
    expect(cleanHarnessPreview("<command-name>red-team</command-name><command-args>audit the API</command-args>")).toBe("/red-team audit the API");
    expect(cleanHarnessPreview("<task-notification><status>failed</status><summary>build broke</summary></task-notification>")).toBe("⚙ task failed — build broke");
    expect(cleanHarnessPreview("<system-reminder>x</system-reminder>")).toBe("system reminder");
    expect(cleanHarnessPreview("just a normal prompt")).toBe("just a normal prompt");
  });

  it("collapses a multi-line command-args to its first non-empty line", () => {
    scenario(
      () => "<command-name>loop</command-name><command-args>\n\n  first line\nsecond line</command-args>",
      (t) => cleanHarnessPreview(t),
      (p) => expect(p).toBe("/loop first line"),
    );
  });
});
