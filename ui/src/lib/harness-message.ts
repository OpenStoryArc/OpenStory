/** Recognize and humanize harness-injected wrapper messages.
 *
 *  Agent transcripts contain user "messages" that are really harness plumbing:
 *  slash-command invocations (`<command-name>` / `<command-message>` /
 *  `<command-args>`), background task notifications (`<task-notification>`),
 *  system reminders (`<system-reminder>`), and local command output
 *  (`<local-command-stdout>`). Rendered raw + truncated they're unreadable
 *  noise. This pure classifier turns them into a clean, structured form for
 *  display — the raw payload is never mutated; this is a view-layer transform.
 */

export type HarnessMessage =
  | { kind: "slash_command"; command: string; args: string; stdout: string | null }
  | { kind: "task_notification"; status: string | null; summary: string | null; taskId: string | null }
  | { kind: "system_reminder"; body: string }
  | { kind: "local_stdout"; body: string }
  | { kind: "plain"; text: string };

function tag(text: string, name: string): string | null {
  // Tolerate truncated input: match an open tag even if the close is missing.
  const closed = new RegExp(`<${name}>([\\s\\S]*?)</${name}>`).exec(text);
  if (closed) return closed[1]!.trim();
  const open = new RegExp(`<${name}>([\\s\\S]*)$`).exec(text);
  if (open) return open[1]!.trim();
  return null;
}

/** Classify a user-message string. Returns `plain` when no wrapper is present. */
export function classifyHarnessMessage(text: string): HarnessMessage {
  const t = text ?? "";

  if (t.includes("<task-notification>")) {
    return {
      kind: "task_notification",
      status: tag(t, "status"),
      summary: tag(t, "summary"),
      taskId: tag(t, "task-id"),
    };
  }

  if (t.includes("<command-name>") || t.includes("<command-message>")) {
    // Prefer command-name; fall back to command-message (survives truncation).
    const command = tag(t, "command-name") || tag(t, "command-message") || "";
    return {
      kind: "slash_command",
      command: command.replace(/^\//, ""),
      args: tag(t, "command-args") ?? "",
      stdout: tag(t, "local-command-stdout"),
    };
  }

  if (t.includes("<local-command-stdout>")) {
    return { kind: "local_stdout", body: tag(t, "local-command-stdout") ?? "" };
  }

  if (t.includes("<system-reminder>")) {
    return { kind: "system_reminder", body: tag(t, "system-reminder") ?? "" };
  }

  return { kind: "plain", text: t };
}

/** True when the text is a harness wrapper (not an ordinary human message). */
export function isHarnessMessage(text: string): boolean {
  return classifyHarnessMessage(text).kind !== "plain";
}

/** How a user message should render: a slash-command becomes a clean `command`
 *  chip (no raw <command-*> tags); everything else becomes a markdown `body`.
 *  Pure view-decision so the message component stays a thin renderer. */
export function userMessageView(text: string): { command: string | null; body: string } {
  const m = classifyHarnessMessage(text);
  switch (m.kind) {
    case "slash_command":
      return { command: m.args ? `/${m.command} ${m.args}` : `/${m.command}`, body: "" };
    case "system_reminder":
      return { command: null, body: m.body };
    case "task_notification":
      return { command: null, body: m.summary ?? "" };
    case "local_stdout":
      return { command: null, body: m.body };
    case "plain":
      return { command: null, body: m.text };
  }
}

function firstLine(s: string, max = 80): string {
  const line = s.split("\n").map((l) => l.trim()).find((l) => l.length > 0) ?? "";
  return line.length > max ? line.slice(0, max - 1) + "…" : line;
}

/**
 * A clean, single-line preview for lists, labels, and collapsed rows.
 * Plain messages are returned as-is (callers still cap length as needed).
 */
export function cleanHarnessPreview(text: string): string {
  const m = classifyHarnessMessage(text);
  switch (m.kind) {
    case "slash_command":
      return m.args ? `/${m.command} ${firstLine(m.args)}` : `/${m.command}`;
    case "task_notification": {
      const parts = ["⚙ task"];
      if (m.status) parts.push(m.status);
      const head = parts.join(" ");
      return m.summary ? `${head} — ${firstLine(m.summary)}` : head;
    }
    case "local_stdout":
      return `$ ${firstLine(m.body)}`;
    case "system_reminder":
      return "system reminder";
    case "plain":
      // Upstream truncation can leave a dangling harness tag fragment (e.g. a
      // bare "</command-message>") on an otherwise-plain label. Strip only the
      // known harness tags so real "<" / ">" in prose survive.
      return m.text.replace(/<\/?(?:command-(?:message|name|args)|local-command-stdout|system-reminder|task-notification|status|summary|task-id)>/g, "").trim();
  }
}
